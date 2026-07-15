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
    this.delegatedSigning,
  });

  final MessageTarget target;
  final String text;
  final bool markdown;
  final MessageSecurityMode security;
  final String? clientMessageId;
  final String? idempotencyKey;
  final bool waitForFinalAcceptance;
  final DelegatedSigningOptions? delegatedSigning;
}

class SendPayloadRequest {
  const SendPayloadRequest({
    required this.target,
    required this.payloadJson,
    this.security = MessageSecurityMode.secureDirect,
    this.clientMessageId,
    this.idempotencyKey,
    this.waitForFinalAcceptance = false,
    this.delegatedSigning,
  });

  final MessageTarget target;
  final String payloadJson;
  final MessageSecurityMode security;
  final String? clientMessageId;
  final String? idempotencyKey;
  final bool waitForFinalAcceptance;
  final DelegatedSigningOptions? delegatedSigning;
}

class SendConversationTextRequest {
  const SendConversationTextRequest({
    required this.conversation,
    required this.text,
    this.markdown = false,
    this.security = MessageSecurityMode.defaultPlain,
    this.clientMessageId,
    this.idempotencyKey,
    this.waitForFinalAcceptance = false,
    this.delegatedSigning,
  });

  final ConversationReadRef conversation;
  final String text;
  final bool markdown;
  final MessageSecurityMode security;
  final String? clientMessageId;
  final String? idempotencyKey;
  final bool waitForFinalAcceptance;
  final DelegatedSigningOptions? delegatedSigning;
}

class SendConversationPayloadRequest {
  const SendConversationPayloadRequest({
    required this.conversation,
    required this.payloadJson,
    this.security = MessageSecurityMode.defaultPlain,
    this.clientMessageId,
    this.idempotencyKey,
    this.waitForFinalAcceptance = false,
    this.delegatedSigning,
  });

  final ConversationReadRef conversation;
  final String payloadJson;
  final MessageSecurityMode security;
  final String? clientMessageId;
  final String? idempotencyKey;
  final bool waitForFinalAcceptance;
  final DelegatedSigningOptions? delegatedSigning;
}

class DelegatedSigningOptions {
  const DelegatedSigningOptions({
    this.logicalSenderDid,
    this.signingVerificationMethod,
    this.signingKeyRef,
    this.actorAgentDid,
  });

  final String? logicalSenderDid;
  final String? signingVerificationMethod;
  final String? signingKeyRef;
  final String? actorAgentDid;
}

class InboxHistoryOptions {
  const InboxHistoryOptions({
    this.inboxOwnerDid,
    this.inboxAuthVerificationMethod,
    this.inboxAuthKeyRef,
    this.inboxAuth,
  });

  final String? inboxOwnerDid;
  final String? inboxAuthVerificationMethod;
  final String? inboxAuthKeyRef;
  final InboxAuth? inboxAuth;
}

sealed class InboxAuth {
  const InboxAuth();

  const factory InboxAuth.scopedInboxToken(ScopedInboxToken token) =
      ScopedInboxTokenAuth;
}

class ScopedInboxTokenAuth extends InboxAuth {
  const ScopedInboxTokenAuth(this.token);

  final ScopedInboxToken token;
}

class ScopedInboxToken {
  const ScopedInboxToken({required this.token});

  final String token;
}

class MessageBodyView {
  const MessageBodyView({
    this.text,
    this.kind,
    this.payloadJson,
    this.unsupportedContentType,
  });

  final String? text;
  final String? kind;
  final String? payloadJson;
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
    this.conversationIdentity,
    this.attributes = const [],
  });

  final String? operationId;
  final String? deliveryState;
  final String? sendState;
  final bool? retryable;
  final String? retryAction;
  final int? serverSequence;
  final String? contentType;
  final ConversationIdentity? conversationIdentity;
  final List<MessageMetadataAttribute> attributes;
}

class ConversationIdentity {
  const ConversationIdentity({
    required this.conversationId,
    required this.canonicalThreadKind,
    required this.canonicalThreadId,
    required this.storageThreadRef,
    this.aliases = const [],
    required this.identityScope,
    required this.migrationState,
  });

  final String conversationId;
  final String canonicalThreadKind;
  final String canonicalThreadId;
  final ConversationStorageThreadRef storageThreadRef;
  final List<ConversationAlias> aliases;
  final ConversationIdentityScope identityScope;
  final ConversationMigrationState migrationState;
}

class ConversationStorageThreadRef {
  const ConversationStorageThreadRef({required this.kind, required this.id});

  final String kind;
  final String id;
}

class ConversationAlias {
  const ConversationAlias({
    required this.kind,
    required this.id,
    required this.source,
  });

  final String kind;
  final String id;
  final ConversationAliasSource source;
}

enum ConversationAliasSource {
  legacyDirectDid,
  oldFlutterSortedDirect,
  peerScopeStorage,
  groupStorage,
  threadStorage,
  unknown,
}

enum ConversationIdentityScope { direct, group, thread, mail, unknown }

enum ConversationMigrationState {
  canonical,
  aliasResolved,
  legacyInput,
  unknown,
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

class SyncDeltaRequest {
  const SyncDeltaRequest({this.limit, this.deviceId, this.reason});

  final int? limit;
  final String? deviceId;
  final String? reason;
}

class SyncDeltaResult {
  const SyncDeltaResult({
    required this.eventsApplied,
    required this.pagesFetched,
    this.lastAppliedEventSeq,
    required this.hasMore,
    required this.snapshotRequired,
    this.retentionFloorEventSeq,
    this.warnings = const [],
  });

  final int eventsApplied;
  final int pagesFetched;
  final String? lastAppliedEventSeq;
  final bool hasMore;
  final bool snapshotRequired;
  final String? retentionFloorEventSeq;
  final List<String> warnings;
}

class ConversationListSnapshot {
  const ConversationListSnapshot({
    required this.formatVersion,
    required this.imSchemaVersion,
    required this.ownerIdentityId,
    required this.ownerDid,
    required this.generatedAtMs,
    this.summaryVersion,
    required this.unreadTotal,
    required this.items,
  });

  final int formatVersion;
  final int imSchemaVersion;
  final String ownerIdentityId;
  final String ownerDid;
  final int generatedAtMs;
  final String? summaryVersion;
  final int unreadTotal;
  final List<ConversationSnapshotItem> items;
}

enum ConversationStorePatchKind {
  reset,
  upsert,
  remove,
  reorder,
  repairRequired,
}

class ConversationStorePatch {
  const ConversationStorePatch({
    required this.kind,
    required this.ownerIdentityId,
    required this.ownerDid,
    required this.version,
    required this.unreadTotal,
    this.items = const [],
    this.item,
    this.index,
    this.threadKind,
    this.threadId,
    this.conversationIdentity,
    this.reason,
  });

  final ConversationStorePatchKind kind;
  final String ownerIdentityId;
  final String ownerDid;
  final int version;
  final int unreadTotal;
  final List<ConversationSnapshotItem> items;
  final ConversationSnapshotItem? item;
  final int? index;
  final String? threadKind;
  final String? threadId;
  final ConversationIdentity? conversationIdentity;
  final String? reason;
}

enum ThreadMessageStorePatchKind { reset, upsert, remove, repairRequired }

class ThreadMessageStorePatch {
  const ThreadMessageStorePatch({
    required this.kind,
    required this.ownerIdentityId,
    required this.ownerDid,
    required this.version,
    required this.threadKind,
    required this.threadId,
    this.items = const [],
    this.message,
    this.index,
    this.conversationIdentity,
    this.messageId,
    this.reason,
  });

  final ThreadMessageStorePatchKind kind;
  final String ownerIdentityId;
  final String ownerDid;
  final int version;
  final String threadKind;
  final String threadId;
  final List<Message> items;
  final Message? message;
  final int? index;
  final ConversationIdentity? conversationIdentity;
  final String? messageId;
  final String? reason;
}

class ConversationSnapshotItem {
  const ConversationSnapshotItem({
    required this.threadKind,
    required this.threadId,
    this.conversationIdentity,
    this.participants = const [],
    this.lastMessage,
    required this.unreadCount,
    this.unreadMentionCount = 0,
    this.firstUnreadMentionMessageId,
    required this.messageCount,
    this.lastMessageAt,
    this.activityAt,
  });

  final String threadKind;
  final String threadId;
  final ConversationIdentity? conversationIdentity;
  final List<String> participants;
  final ConversationSnapshotMessage? lastMessage;
  final int unreadCount;
  final int unreadMentionCount;
  final String? firstUnreadMentionMessageId;
  final int messageCount;
  final String? lastMessageAt;
  final String? activityAt;
}

class ConversationSnapshotMessage {
  const ConversationSnapshotMessage({
    required this.id,
    required this.threadKind,
    required this.threadId,
    this.conversationIdentity,
    required this.direction,
    required this.sender,
    this.receiver,
    this.group,
    required this.body,
    this.sentAt,
    this.receivedAt,
    this.serverSequence,
    this.contentType,
    this.attributes = const [],
  });

  final String id;
  final String threadKind;
  final String threadId;
  final ConversationIdentity? conversationIdentity;
  final String direction;
  final String sender;
  final String? receiver;
  final String? group;
  final ConversationSnapshotMessageBody body;
  final String? sentAt;
  final String? receivedAt;
  final int? serverSequence;
  final String? contentType;
  final List<MessageMetadataAttribute> attributes;
}

class ConversationSnapshotMessageBody {
  const ConversationSnapshotMessageBody({
    this.text,
    this.kind,
    this.payloadJson,
    this.unsupportedContentType,
  });

  final String? text;
  final String? kind;
  final String? payloadJson;
  final String? unsupportedContentType;
}

class ConversationReadRef {
  const ConversationReadRef({required this.conversationId});

  final String conversationId;
}

class SyncThreadAfterRequest {
  const SyncThreadAfterRequest({
    required this.thread,
    this.afterServerSeq,
    this.limit,
  });

  final ThreadRef thread;
  final String? afterServerSeq;
  final int? limit;
}

class SyncConversationAfterRequest {
  const SyncConversationAfterRequest({
    required this.conversation,
    this.afterServerSeq,
    this.limit,
  });

  final ConversationReadRef conversation;
  final String? afterServerSeq;
  final int? limit;
}

class SyncThreadAfterResult {
  const SyncThreadAfterResult({
    required this.messages,
    this.nextAfterServerSeq,
    required this.hasMore,
    this.warnings = const [],
  });

  final List<Message> messages;
  final String? nextAfterServerSeq;
  final bool hasMore;
  final List<String> warnings;
}

class Conversation {
  const Conversation({
    required this.threadKind,
    required this.threadId,
    this.conversationIdentity,
    this.title,
    this.participants = const [],
    this.lastMessage,
    required this.unreadCount,
    this.unreadMentionCount = 0,
    this.firstUnreadMentionMessageId,
    required this.messageCount,
    this.lastMessageAt,
    this.activityAt,
  });

  final String threadKind;
  final String threadId;
  final ConversationIdentity? conversationIdentity;
  final String? title;
  final List<String> participants;
  final Message? lastMessage;
  final int unreadCount;
  final int unreadMentionCount;
  final String? firstUnreadMentionMessageId;
  final int messageCount;
  final String? lastMessageAt;
  final String? activityAt;
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

class MarkThreadReadResult {
  const MarkThreadReadResult({
    required this.updatedCount,
    required this.remoteAcknowledged,
    required this.partial,
    required this.fallbackUsed,
    required this.pendingRemoteAck,
    this.effectiveWatermark,
    this.legacyMessageIds = const [],
    this.warnings = const [],
  });

  final int updatedCount;
  final bool remoteAcknowledged;
  final bool partial;
  final bool fallbackUsed;
  final bool pendingRemoteAck;
  final ReadWatermark? effectiveWatermark;
  final List<String> legacyMessageIds;
  final List<String> warnings;
}

class ReadWatermark {
  const ReadWatermark({
    this.lastReadMessageId,
    this.lastReadThreadSeq,
    this.readAt,
  });

  final String? lastReadMessageId;
  final String? lastReadThreadSeq;
  final DateTime? readAt;
}
