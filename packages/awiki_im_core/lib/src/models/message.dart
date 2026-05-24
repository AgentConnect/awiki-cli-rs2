sealed class MessageTarget {
  const MessageTarget();

  const factory MessageTarget.direct(String peer) = DirectMessageTarget;
  const factory MessageTarget.group(String group) = GroupMessageTarget;
}

class DirectMessageTarget extends MessageTarget {
  const DirectMessageTarget(this.peer);
  final String peer;
}

class GroupMessageTarget extends MessageTarget {
  const GroupMessageTarget(this.group);
  final String group;
}

sealed class ThreadRef {
  const ThreadRef();

  const factory ThreadRef.direct(String peer) = DirectThreadRef;
  const factory ThreadRef.group(String group) = GroupThreadRef;
  const factory ThreadRef.thread(String threadId) = MessageThreadRef;
}

class DirectThreadRef extends ThreadRef {
  const DirectThreadRef(this.peer);
  final String peer;
}

class GroupThreadRef extends ThreadRef {
  const GroupThreadRef(this.group);
  final String group;
}

class MessageThreadRef extends ThreadRef {
  const MessageThreadRef(this.threadId);
  final String threadId;
}

enum MessageSecurityMode {
  defaultPlain,
  plain,
  e2eeRequired,
  secureDirect,
  groupE2ee,
}

enum MessageDirection { outgoing, incoming, unknown }

class SendTextRequest {
  const SendTextRequest({
    required this.target,
    required this.text,
    this.markdown = false,
    this.security = MessageSecurityMode.defaultPlain,
    this.clientMessageId,
    this.idempotencyKey,
    this.waitForFinalAcceptance = false,
  });

  final MessageTarget target;
  final String text;
  final bool markdown;
  final MessageSecurityMode security;
  final String? clientMessageId;
  final String? idempotencyKey;
  final bool waitForFinalAcceptance;
}

class MessageBodyView {
  const MessageBodyView({this.text, this.kind, this.unsupportedContentType});

  final String? text;
  final String? kind;
  final String? unsupportedContentType;
}

class MessageMetadataAttribute {
  const MessageMetadataAttribute({required this.key, required this.value});

  final String key;
  final String value;
}

class MessageMetadata {
  const MessageMetadata({
    this.operationId,
    this.deliveryState,
    this.sendState,
    this.retryable,
    this.retryAction,
    this.serverSequence,
    this.contentType,
    this.attributes = const [],
  });

  final String? operationId;
  final String? deliveryState;
  final String? sendState;
  final bool? retryable;
  final String? retryAction;
  final int? serverSequence;
  final String? contentType;
  final List<MessageMetadataAttribute> attributes;
}

class Message {
  const Message({
    required this.id,
    required this.threadKind,
    required this.threadId,
    required this.direction,
    required this.sender,
    this.receiver,
    this.group,
    required this.body,
    this.sentAt,
    this.receivedAt,
    required this.metadata,
  });

  final String id;
  final String threadKind;
  final String threadId;
  final MessageDirection direction;
  final String sender;
  final String? receiver;
  final String? group;
  final MessageBodyView body;
  final String? sentAt;
  final String? receivedAt;
  final MessageMetadata metadata;
}

class MessagePage {
  const MessagePage({
    required this.items,
    this.nextCursor,
    required this.hasMore,
  });

  final List<Message> items;
  final String? nextCursor;
  final bool hasMore;
}

class Conversation {
  const Conversation({
    required this.threadKind,
    required this.threadId,
    this.title,
    this.participants = const [],
    this.lastMessage,
    required this.unreadCount,
    required this.messageCount,
    this.lastMessageAt,
  });

  final String threadKind;
  final String threadId;
  final String? title;
  final List<String> participants;
  final Message? lastMessage;
  final int unreadCount;
  final int messageCount;
  final String? lastMessageAt;
}

class ConversationPage {
  const ConversationPage({
    required this.items,
    this.nextCursor,
    required this.hasMore,
  });

  final List<Conversation> items;
  final String? nextCursor;
  final bool hasMore;
}

class SendMessageResult {
  const SendMessageResult({
    required this.message,
    required this.deliveryState,
    this.warnings = const [],
  });

  final Message message;
  final String deliveryState;
  final List<String> warnings;
}

class MarkReadResult {
  const MarkReadResult({
    required this.updatedCount,
    this.messageIds = const [],
    this.warnings = const [],
  });

  final int updatedCount;
  final List<String> messageIds;
  final List<String> warnings;
}
