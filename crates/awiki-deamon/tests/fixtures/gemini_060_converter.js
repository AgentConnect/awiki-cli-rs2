// Gemini CLI 0.60.0, Apache-2.0, Google LLC.
// Exact bundled converter for compatibility fingerprint regression.
function ensurePartArray(content) {
  return (Array.isArray(content) ? content : [content]).map(p => typeof p === 'string' ? {text:p} : p);
}
function partListUnionToString(value) { return typeof value === 'string' ? value : JSON.stringify(value); }
function isIgnoredUserContent(text) { return !text || text.startsWith('/') || text.startsWith('?') || text.includes('<session_context>'); }
function convertSessionToClientHistory(messages) {
  const clientHistory = [];
  for (const msg of messages) {
    if (msg.type === "info" || msg.type === "error" || msg.type === "warning") {
      continue;
    }
    if (msg.type === "user") {
      const contentString = partListUnionToString(msg.content);
      const trimmedContent = contentString.trim();
      if (isIgnoredUserContent(trimmedContent)) {
        continue;
      }
      clientHistory.push({
        id: msg.id,
        content: {
          role: "user",
          parts: ensurePartArray(msg.content)
        }
      });
    } else if (msg.type === "gemini") {
      const modelParts = [];
      const contentParts = msg.content ? ensurePartArray(msg.content) : [];
      const hasCallsInContent = contentParts.some((p) => !!p.functionCall);
      const hasThoughtsInContent = contentParts.some((p) => p.thought);
      if (hasCallsInContent || hasThoughtsInContent) {
        modelParts.push(...contentParts);
      } else {
        if (msg.thoughts && msg.thoughts.length > 0) {
          for (const thought of msg.thoughts) {
            const thoughtText = thought.subject ? `**${thought.subject}** ${thought.description}` : thought.description;
            modelParts.push({
              text: thoughtText,
              thought: true
            });
          }
        }
        modelParts.push(...contentParts);
        if (msg.toolCalls && msg.toolCalls.length > 0) {
          for (const toolCall of msg.toolCalls) {
            modelParts.push({
              functionCall: {
                id: toolCall.id,
                name: toolCall.name,
                args: toolCall.args
              }
            });
          }
        }
      }
      if (modelParts.length > 0) {
        clientHistory.push({
          id: msg.id,
          content: {
            role: "model",
            parts: modelParts
          }
        });
        if (msg.toolCalls && msg.toolCalls.length > 0) {
          const functionResponseParts = [];
          for (const toolCall of msg.toolCalls) {
            if (toolCall.result) {
              let responseData;
              if (typeof toolCall.result === "string") {
                responseData = {
                  functionResponse: {
                    id: toolCall.id,
                    name: toolCall.name,
                    response: {
                      output: toolCall.result
                    }
                  }
                };
              } else if (Array.isArray(toolCall.result)) {
                functionResponseParts.push(...ensurePartArray(toolCall.result));
                continue;
              } else {
                responseData = toolCall.result;
              }
              functionResponseParts.push(responseData);
            }
          }
          if (functionResponseParts.length > 0) {
            clientHistory.push({
              id: `${msg.id}_response`,
              content: {
                role: "user",
                parts: functionResponseParts
              }
            });
          }
        }
      }
    }
  }
  return clientHistory;
}

// End upstream converter.
export { convertSessionToClientHistory };
export function recording(model, responseText, consolidatedParts) {
  let id;
      id = this.chatRecordingService.recordMessage({
        model,
        type: "gemini",
        content: responseText
      });
  return id;
}
