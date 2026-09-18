"""Opt-in test callback: log model routing facts only, never request content."""
import json
import os
from litellm.integrations.custom_logger import CustomLogger


def route_record(kwargs, response):
    params = kwargs.get('litellm_params') or {}
    # LiteLLM's Google endpoints use litellm_metadata; completion endpoints
    # retain metadata. Prefer the same field as LiteLLM's metadata helper.
    metadata = params.get('litellm_metadata') or params.get('metadata') or {}
    return {'requested': metadata.get('model_group'), 'model': kwargs.get('model'),
            'provider': params.get('custom_llm_provider'), 'response_model': getattr(response, 'model', None)}


class ModelAudit(CustomLogger):
    async def async_log_success_event(self, kwargs, response_obj, start_time, end_time):
        path = os.environ.get('AWIKI_MODEL_AUDIT_PATH')
        if not path:
            return
        with open(path, 'a', opener=lambda p, f: os.open(p, f, 0o600)) as stream:
            stream.write(json.dumps(route_record(kwargs, response_obj)) + '\n')


audit = ModelAudit()
