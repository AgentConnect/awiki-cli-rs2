"""Local Gemini-to-DeepSeek interoperability for official LiteLLM 1.101.0.

Preserves system instructions, inline images, tool call identity and all tool
results. Unsupported content fails explicitly. DeepSeek thinking is disabled
for this route because the Google adapter does not round-trip reasoning_content.
Official installed packages are unchanged. Recheck when upgrading LiteLLM.
"""
from __future__ import annotations

import hashlib
import json
from typing import Any


def google_messages(contents: list[dict[str, Any]], system_instruction=None):
    messages: list[dict[str, Any]] = []
    if system_instruction:
        parts = system_instruction.get("parts", [])
        if any(not isinstance(p, dict) or set(p) != {"text"} for p in parts):
            raise ValueError("unsupported_system_content")
        messages.append({"role": "system", "content": "\n".join(p["text"] for p in parts)})
    calls: dict[str, dict[str, Any]] = {}
    results: dict[str, list[Any]] = {}
    tool_messages: dict[str, dict[str, Any]] = {}
    seen_ids: set[str] = set()

    def complete_group():
        if calls.keys() - results.keys():
            raise ValueError("missing_tool_result")
        calls.clear()
        results.clear()
        tool_messages.clear()

    for content in contents:
        role = content.get("role", "user")
        if role not in {"user", "model"}:
            raise ValueError("unsupported_content_role")
        texts, functions, responses = [], [], []
        for part in content.get("parts", []):
            if isinstance(part, str):
                texts.append({"type": "text", "text": part})
                continue
            if not isinstance(part, dict):
                raise ValueError("unsupported_content_part")
            if set(part) - {"text", "inlineData", "inline_data", "functionCall", "functionResponse", "thoughtSignature", "thought"}:
                raise ValueError("unsupported_content_part")
            if "text" in part:
                texts.append({"type": "text", "text": part["text"]})
            if "inlineData" in part or "inline_data" in part:
                data = part.get("inlineData", part.get("inline_data"))
                mime = data.get("mimeType", data.get("mime_type"))
                if role != "user" or not mime or not data.get("data"):
                    raise ValueError("unsupported_inline_data")
                texts.append({"type": "image_url", "image_url": {"url": f"data:{mime};base64,{data['data']}"}})
            if "functionCall" in part:
                functions.append(part["functionCall"])
            if "functionResponse" in part:
                responses.append(part["functionResponse"])
        if role == "model":
            if responses or any(p["type"] != "text" for p in texts):
                raise ValueError("unsupported_model_content")
            text = "".join(p["text"] for p in texts)
            if not text and not functions:
                continue  # Empty native recording checkpoint, no content.
            complete_group()
            message: dict[str, Any] = {"role": "assistant", "content": text or None}
            if functions:
                converted = []
                for function in functions:
                    original_id = function.get("id")
                    if not isinstance(original_id, str) or not original_id:
                        raise ValueError("missing_function_call_id")
                    if original_id in seen_ids:
                        raise ValueError("duplicate_function_call_id")
                    seen_ids.add(original_id)
                    call_id = "call_" + hashlib.sha256(original_id.encode()).hexdigest()[:24]
                    calls[original_id] = {"id": call_id, "name": function["name"]}
                    converted.append({"id": call_id, "type": "function", "function": {
                        "name": function["name"], "arguments": json.dumps(function.get("args", {})),
                    }})
                message["tool_calls"] = converted
            messages.append(message)
        else:
            if functions:
                raise ValueError("unsupported_user_function_call")
            for response in responses:
                identifier = response.get("id")
                if identifier not in calls:
                    raise ValueError("ambiguous_or_orphan_tool_result")
                if response.get("name") != calls[identifier]["name"]:
                    raise ValueError("tool_result_name_mismatch")
                values = results.setdefault(identifier, [])
                values.append(response.get("response", {}))
                if identifier not in tool_messages:
                    tool_messages[identifier] = {"role": "tool", "tool_call_id": calls[identifier]["id"]}
                    messages.append(tool_messages[identifier])
                # Multiple native records for one call are all preserved.
                tool_messages[identifier]["content"] = json.dumps(values[0] if len(values) == 1 else {"results": values})
            if texts:
                complete_group()
                messages.append({"role": "user", "content": texts[0]["text"] if len(texts) == 1 and texts[0]["type"] == "text" else texts})
    if calls.keys() - results.keys():
        raise ValueError("missing_tool_result")
    return messages


def install_compatibility():
    from importlib.metadata import version
    if version("litellm") != "1.101.0":
        raise RuntimeError("Gemini/DeepSeek compatibility requires LiteLLM 1.101.0; verify compatibility before upgrading")
    from litellm.google_genai.adapters.transformation import GoogleGenAIAdapter
    if getattr(GoogleGenAIAdapter, "_awiki_deepseek_compatibility", False):
        return
    from litellm.exceptions import BadRequestError

    class HistoryBadRequest(BadRequestError):
        def __init__(self, code):
            self.history_code = code
            super().__init__(message=f"gemini_invalid_history:{code}",
                             model="gemini-deepseek-relay", llm_provider="deepseek")

    def transform(self, contents, system_instruction=None):
        try:
            return google_messages(contents, system_instruction)
        except (ValueError, TypeError, KeyError, AttributeError) as error:
            # A deterministic request-shape/history failure must be HTTP 400,
            # including before the first streaming chunk. Never expose contents
            # or retry this as an upstream transport failure.
            known_codes = {
                "unsupported_system_content", "missing_tool_result",
                "unsupported_content_role", "unsupported_content_part",
                "unsupported_inline_data", "unsupported_model_content",
                "missing_function_call_id", "duplicate_function_call_id",
                "unsupported_user_function_call", "ambiguous_or_orphan_tool_result",
                "tool_result_name_mismatch",
            }
            code = str(error) if isinstance(error, ValueError) and str(error) in known_codes else "invalid_content_shape"
            raise HistoryBadRequest(code) from None

    GoogleGenAIAdapter._transform_contents_to_messages = transform
    original = GoogleGenAIAdapter.translate_generate_content_to_completion

    def translate(self, *args, **kwargs):
        result = original(self, *args, **kwargs)
        result["thinking"] = {"type": "disabled"}
        return result
    GoogleGenAIAdapter.translate_generate_content_to_completion = translate
    GoogleGenAIAdapter._awiki_history_error = HistoryBadRequest
    GoogleGenAIAdapter._awiki_deepseek_compatibility = True


def install_proxy_error_handler():
    # Unlike generate/stream, LiteLLM 1.101 countTokens bypasses its common
    # exception mapper. Register only our deterministic conversion exception.
    # Do not relabel unrelated proxy or upstream failures.
    install_compatibility()
    from litellm.google_genai.adapters.transformation import GoogleGenAIAdapter
    from litellm.proxy.proxy_server import app
    from starlette.responses import JSONResponse

    async def invalid_history(request, error):
        return JSONResponse(status_code=400, content={"error": {
            "code": 400, "status": "INVALID_ARGUMENT",
            "message": f"gemini_invalid_history:{error.history_code}",
        }})

    app.add_exception_handler(GoogleGenAIAdapter._awiki_history_error, invalid_history)
