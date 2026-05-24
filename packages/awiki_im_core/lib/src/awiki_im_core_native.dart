import 'dart:async';
import 'dart:typed_data';

import 'generated/api/auth.dart' as gen_auth;
import 'generated/api/attachments.dart' as gen_attachments;
import 'generated/api/client.dart' as gen_client;
import 'generated/api/core.dart' as gen_core;
import 'generated/api/directory.dart' as gen_directory;
import 'generated/api/email.dart' as gen_email;
import 'generated/api/groups.dart' as gen_groups;
import 'generated/api/identity.dart' as gen_identity_api;
import 'generated/api/messages.dart' as gen_messages;
import 'generated/api/profile.dart' as gen_profile;
import 'generated/api/realtime.dart' as gen_realtime;
import 'generated/dto/auth.dart' as gen_auth_dto;
import 'generated/dto/attachment.dart' as gen_attachment;
import 'generated/dto/config.dart' as gen_config;
import 'generated/dto/directory.dart' as gen_directory_dto;
import 'generated/dto/email.dart' as gen_email_dto;
import 'generated/dto/error.dart' as gen_error;
import 'generated/dto/group.dart' as gen_group_dto;
import 'generated/dto/identity.dart' as gen_identity;
import 'generated/dto/message.dart' as gen_message;
import 'generated/dto/profile.dart' as gen_profile_dto;
import 'generated/dto/realtime.dart' as gen_realtime_dto;
import 'generated/frb_generated.dart' as gen;
import 'models/auth.dart';
import 'models/attachment.dart';
import 'models/config.dart';
import 'models/directory.dart';
import 'models/email.dart';
import 'models/error.dart';
import 'models/group.dart';
import 'models/identity.dart';
import 'models/message.dart';
import 'models/profile.dart';
import 'models/realtime.dart';
import 'native_library_loader.dart';

bool _rustLibInitialized = false;

Future<void> _ensureRustLibInitialized() async {
  if (_rustLibInitialized) return;
  await gen.RustLib.init(externalLibrary: loadAwikiImCoreExternalLibrary());
  _rustLibInitialized = true;
}

Future<T> _mapNativeErrors<T>(Future<T> Function() action) async {
  try {
    return await action();
  } on gen_error.DartImError catch (error) {
    throw AwikiImCoreException(
      code: error.code,
      message: error.message,
      field: error.field,
      statusCode: error.statusCode,
      capability: error.capability,
    );
  }
}

class AwikiImCore {
  AwikiImCore._(this._inner);

  final gen_client.ArcDartImCore _inner;
  bool _disposed = false;

  static Future<AwikiImCore> open({
    required AwikiImCoreConfig config,
    required AwikiImCorePaths paths,
  }) async {
    await _ensureRustLibInitialized();
    final inner = await _mapNativeErrors(
      () => gen_core.openCore(config: config._toGen(), paths: paths._toGen()),
    );
    return AwikiImCore._(inner);
  }

  Future<AwikiImClient> client(IdentitySelector selector) async {
    _ensureNotDisposed();
    final inner = await _mapNativeErrors(
      () => gen_client.coreClient(core: _inner, selector: selector._toGen()),
    );
    return AwikiImClient._(inner);
  }

  Future<List<String>> validatePaths() async {
    _ensureNotDisposed();
    return _mapNativeErrors(() => gen_core.validatePaths(core: _inner));
  }

  Future<List<IdentitySummary>> listIdentities() async {
    _ensureNotDisposed();
    final identities = await _mapNativeErrors(
      () => gen_identity_api.listIdentities(core: _inner),
    );
    return identities.map((identity) => identity._toModel()).toList();
  }

  Future<IdentitySummary?> defaultIdentity() async {
    _ensureNotDisposed();
    final identity = await _mapNativeErrors(
      () => gen_identity_api.defaultIdentity(core: _inner),
    );
    return identity?._toModel();
  }

  Future<IdentitySummary> resolveIdentity(IdentitySelector selector) async {
    _ensureNotDisposed();
    final identity = await _mapNativeErrors(
      () => gen_identity_api.resolveIdentity(
        core: _inner,
        selector: selector._toGen(),
      ),
    );
    return identity._toModel();
  }

  Future<HandleRegistrationResult> registerHandleWithPhone({
    String? localAlias,
    required String requestedHandle,
    required String phone,
    String? otp,
    String? inviteCode,
    InitialProfile profile = const InitialProfile(),
    bool makeDefault = true,
  }) async {
    _ensureNotDisposed();
    final result = await _mapNativeErrors(
      () => gen_identity_api.registerHandleWithPhone(
        core: _inner,
        localAlias: localAlias,
        requestedHandle: requestedHandle,
        phone: phone,
        otp: otp,
        inviteCode: inviteCode,
        profile: profile._toGen(),
        makeDefault: makeDefault,
      ),
    );
    return result._toModel();
  }

  Future<HandleRegistrationResult> registerHandleWithEmail({
    String? localAlias,
    required String requestedHandle,
    required String email,
    bool waitForVerification = true,
    String? inviteCode,
    InitialProfile profile = const InitialProfile(),
    bool makeDefault = true,
  }) async {
    _ensureNotDisposed();
    final result = await _mapNativeErrors(
      () => gen_identity_api.registerHandleWithEmail(
        core: _inner,
        localAlias: localAlias,
        requestedHandle: requestedHandle,
        email: email,
        waitForVerification: waitForVerification,
        inviteCode: inviteCode,
        profile: profile._toGen(),
        makeDefault: makeDefault,
      ),
    );
    return result._toModel();
  }

  Future<RecoverHandleResult> recoverHandle({
    required String handle,
    required String phone,
    String? otp,
  }) async {
    _ensureNotDisposed();
    final result = await _mapNativeErrors(
      () => gen_identity_api.recoverHandle(
        core: _inner,
        handle: handle,
        phone: phone,
        otp: otp,
      ),
    );
    return result._toModel();
  }

  Future<void> dispose() async {
    if (_disposed) return;
    await _mapNativeErrors(() => gen_core.closeCore(core: _inner));
    _disposed = true;
  }

  void _ensureNotDisposed() {
    if (_disposed) {
      throw const AwikiImCoreException(
        code: 'object_closed',
        message: 'core disposed',
      );
    }
  }
}

class AwikiImClient {
  AwikiImClient._(this._inner);

  final gen_attachments.ArcDartImClient _inner;
  final StreamController<RealtimeEvent> _eventsController =
      StreamController<RealtimeEvent>.broadcast();
  final StreamController<RealtimeConnectionState> _connectionStatesController =
      StreamController<RealtimeConnectionState>.broadcast();
  gen_realtime.ArcDartRealtimeSession? _realtimeSession;
  StreamSubscription<RealtimeEvent>? _realtimeEventSubscription;
  bool _disposed = false;

  AuthApi get auth => AuthApi._(this);
  IdentityApi get identity => IdentityApi._(this);
  DirectoryApi get directory => DirectoryApi._(this);
  ProfileApi get profile => ProfileApi._(this);
  MessageApi get messages => MessageApi._(this);
  AttachmentApi get attachments => AttachmentApi._(this);
  EmailApi get email => EmailApi._(this);
  GroupApi get groups => GroupApi._(this);
  RealtimeApi get realtime => RealtimeApi._(this);

  Stream<RealtimeEvent> get events => _eventsController.stream;

  Stream<RealtimeConnectionState> get connectionStates =>
      _connectionStatesController.stream;

  Future<void> dispose() async {
    if (_disposed) return;
    await realtime.stop();
    await _mapNativeErrors(() => gen_client.closeClient(client: _inner));
    await _eventsController.close();
    await _connectionStatesController.close();
    _disposed = true;
  }

  void _ensureNotDisposed() {
    if (_disposed) {
      throw const AwikiImCoreException(
        code: 'object_closed',
        message: 'client disposed',
      );
    }
  }
}

class AuthApi {
  AuthApi._(this._client);

  final AwikiImClient _client;

  Future<AuthStatus> status() async {
    _client._ensureNotDisposed();
    final status = await _mapNativeErrors(
      () => gen_auth.authStatus(client: _client._inner),
    );
    return status._toModel();
  }

  Future<SessionBundle> login() async {
    _client._ensureNotDisposed();
    final bundle = await _mapNativeErrors(
      () => gen_auth.authLogin(client: _client._inner),
    );
    return bundle._toModel();
  }

  Future<SessionBundle> ensureSession(AuthScope scope) async {
    _client._ensureNotDisposed();
    final bundle = await _mapNativeErrors(
      () => gen_auth.authEnsureSession(
        client: _client._inner,
        scope: scope._toGen(),
      ),
    );
    return bundle._toModel();
  }

  Future<SessionUpdate> refreshSession() async {
    _client._ensureNotDisposed();
    final update = await _mapNativeErrors(
      () => gen_auth.authRefreshSession(client: _client._inner),
    );
    return update._toModel();
  }
}

class IdentityApi {
  IdentityApi._(this._client);

  final AwikiImClient _client;

  Future<IdentitySummary> current() async {
    _client._ensureNotDisposed();
    final identity = await _mapNativeErrors(
      () => gen_client.currentIdentity(client: _client._inner),
    );
    return identity._toModel();
  }
}

class DirectoryApi {
  DirectoryApi._(this._client);

  final AwikiImClient _client;

  Future<DirectoryResolution> resolvePeer(String peer) async {
    _client._ensureNotDisposed();
    final resolution = await _mapNativeErrors(
      () => gen_directory.resolvePeer(client: _client._inner, peer: peer),
    );
    return resolution._toModel();
  }

  Future<DirectoryResolution> lookupHandle(String handle) async {
    _client._ensureNotDisposed();
    final resolution = await _mapNativeErrors(
      () => gen_directory.lookupHandle(client: _client._inner, handle: handle),
    );
    return resolution._toModel();
  }

  Future<RelationStatus> relationStatus(String peer) async {
    _client._ensureNotDisposed();
    final status = await _mapNativeErrors(
      () => gen_directory.relationStatus(client: _client._inner, peer: peer),
    );
    return status._toModel();
  }
}

class ProfileApi {
  ProfileApi._(this._client);

  final AwikiImClient _client;

  Future<UserProfile> loadMyProfile() async {
    _client._ensureNotDisposed();
    final profile = await _mapNativeErrors(
      () => gen_profile.loadMyProfile(client: _client._inner),
    );
    return profile._toModel();
  }

  Future<UserProfile> updateProfile(ProfilePatch patch) async {
    _client._ensureNotDisposed();
    final profile = await _mapNativeErrors(
      () => gen_profile.updateProfile(
        client: _client._inner,
        patch: patch._toGen(),
      ),
    );
    return profile._toModel();
  }

  Future<UserProfile> loadPublicProfile(IdentitySubject subject) async {
    _client._ensureNotDisposed();
    final profile = await _mapNativeErrors(
      () => gen_profile.loadPublicProfile(
        client: _client._inner,
        subject: subject._toGen(),
      ),
    );
    return profile._toModel();
  }
}

class MessageApi {
  MessageApi._(this._client);

  final AwikiImClient _client;

  Future<SendMessageResult> sendText(SendTextRequest request) async {
    _client._ensureNotDisposed();
    final result = await _mapNativeErrors(
      () => gen_messages.sendText(
        client: _client._inner,
        request: request._toGen(),
      ),
    );
    return result._toModel();
  }

  Future<MessagePage> inbox({
    required int limit,
    String? cursor,
    bool unreadOnly = false,
  }) async {
    _client._ensureNotDisposed();
    final page = await _mapNativeErrors(
      () => gen_messages.inbox(
        client: _client._inner,
        limit: limit,
        cursor: cursor,
        unreadOnly: unreadOnly,
      ),
    );
    return page._toModel();
  }

  Future<MessagePage> history(
    ThreadRef thread, {
    required int limit,
    String? cursor,
  }) async {
    _client._ensureNotDisposed();
    final page = await _mapNativeErrors(
      () => gen_messages.history(
        client: _client._inner,
        thread: thread._toGen(),
        limit: limit,
        cursor: cursor,
      ),
    );
    return page._toModel();
  }

  Future<MarkReadResult> markRead(List<String> messageIds) async {
    _client._ensureNotDisposed();
    final result = await _mapNativeErrors(
      () =>
          gen_messages.markRead(client: _client._inner, messageIds: messageIds),
    );
    return result._toModel();
  }

  Future<ConversationPage> conversations({
    required int limit,
    bool includeGroups = true,
    bool includeDirect = true,
    bool unreadOnly = false,
  }) async {
    _client._ensureNotDisposed();
    final page = await _mapNativeErrors(
      () => gen_messages.conversations(
        client: _client._inner,
        limit: limit,
        includeGroups: includeGroups,
        includeDirect: includeDirect,
        unreadOnly: unreadOnly,
      ),
    );
    return page._toModel();
  }

  Future<SendMessageResult> retryMessage(String messageId) async {
    _client._ensureNotDisposed();
    final result = await _mapNativeErrors(
      () => gen_messages.retryMessage(
        client: _client._inner,
        messageId: messageId,
      ),
    );
    return result._toModel();
  }
}

class AttachmentApi {
  AttachmentApi._(this._client);

  final AwikiImClient _client;

  Future<SendMessageResult> send(AttachmentSendRequest request) async {
    _client._ensureNotDisposed();
    final result = await _mapNativeErrors(
      () => gen_attachments.sendAttachment(
        client: _client._inner,
        request: request._toGen(),
      ),
    );
    return result._toModel();
  }

  Future<DownloadedAttachment> download(
    DownloadAttachmentRequest request,
  ) async {
    _client._ensureNotDisposed();
    final result = await _mapNativeErrors(
      () => gen_attachments.downloadAttachment(
        client: _client._inner,
        request: request._toGen(),
      ),
    );
    return result._toModel();
  }
}

class EmailApi {
  EmailApi._(this._client);

  final AwikiImClient _client;

  Future<EmailAccount> account() async {
    _client._ensureNotDisposed();
    final account = await _mapNativeErrors(
      () => gen_email.account(client: _client._inner),
    );
    return account._toModel();
  }

  Future<EmailMessageSummaryPage> inbox({
    String folder = 'inbox',
    int limit = 20,
    int offset = 0,
    bool unreadOnly = false,
  }) async {
    _client._ensureNotDisposed();
    final page = await _mapNativeErrors(
      () => gen_email.inbox(
        client: _client._inner,
        folder: folder,
        limit: limit,
        offset: offset,
        unreadOnly: unreadOnly,
      ),
    );
    return page._toModel();
  }

  Future<EmailMessageSummaryPage> inboxWithQuery(EmailInboxQuery query) =>
      inbox(
        folder: query.folder,
        limit: query.limit,
        offset: query.offset,
        unreadOnly: query.unreadOnly,
      );

  Future<EmailMessage> read(String messageId) async {
    _client._ensureNotDisposed();
    final message = await _mapNativeErrors(
      () => gen_email.read(client: _client._inner, messageId: messageId),
    );
    return message._toModel();
  }

  Future<EmailMarkReadResult> markRead(
    List<String> messageIds, {
    bool isRead = true,
  }) async {
    _client._ensureNotDisposed();
    final result = await _mapNativeErrors(
      () => gen_email.markRead(
        client: _client._inner,
        messageIds: messageIds,
        isRead: isRead,
      ),
    );
    return result._toModel();
  }

  Future<SendEmailResult> send(SendEmailRequest request) async {
    _client._ensureNotDisposed();
    final result = await _mapNativeErrors(
      () => gen_email.send(client: _client._inner, request: request._toGen()),
    );
    return result._toModel();
  }

  Future<EmailAttachmentContent> downloadAttachment({
    required String messageId,
    required int attachmentIndex,
  }) async {
    _client._ensureNotDisposed();
    final content = await _mapNativeErrors(
      () => gen_email.downloadAttachment(
        client: _client._inner,
        messageId: messageId,
        attachmentIndex: attachmentIndex,
      ),
    );
    return content._toModel();
  }

  Future<EmailNotificationPage> notifications({int limit = 20}) async {
    _client._ensureNotDisposed();
    final page = await _mapNativeErrors(
      () => gen_email.notifications(client: _client._inner, limit: limit),
    );
    return page._toModel();
  }
}

class GroupApi {
  GroupApi._(this._client);

  final AwikiImClient _client;

  Future<GroupReadResult> createGroup(CreateGroupRequest request) async {
    _client._ensureNotDisposed();
    final result = await _mapNativeErrors(
      () => gen_groups.createGroup(
        client: _client._inner,
        request: request._toGen(),
      ),
    );
    return result._toModel();
  }

  Future<GroupReadResult> joinGroup(String groupDid) async {
    _client._ensureNotDisposed();
    final result = await _mapNativeErrors(
      () => gen_groups.joinGroup(client: _client._inner, groupDid: groupDid),
    );
    return result._toModel();
  }

  Future<GroupReadResult> getGroup(String groupDid) async {
    _client._ensureNotDisposed();
    final result = await _mapNativeErrors(
      () => gen_groups.getGroup(client: _client._inner, groupDid: groupDid),
    );
    return result._toModel();
  }

  Future<GroupReadResult> listGroups({required int limit}) async {
    _client._ensureNotDisposed();
    final result = await _mapNativeErrors(
      () => gen_groups.listGroups(client: _client._inner, limit: limit),
    );
    return result._toModel();
  }

  Future<GroupReadResult> listMembers(
    String groupDid, {
    required int limit,
  }) async {
    _client._ensureNotDisposed();
    final result = await _mapNativeErrors(
      () => gen_groups.listGroupMembers(
        client: _client._inner,
        groupDid: groupDid,
        limit: limit,
      ),
    );
    return result._toModel();
  }

  Future<GroupReadResult> listMessages(
    String groupDid, {
    required int limit,
    String? cursor,
  }) async {
    _client._ensureNotDisposed();
    final result = await _mapNativeErrors(
      () => gen_groups.listGroupMessages(
        client: _client._inner,
        groupDid: groupDid,
        limit: limit,
        cursor: cursor,
      ),
    );
    return result._toModel();
  }

  Future<GroupReadResult> leaveGroup(String groupDid) async {
    _client._ensureNotDisposed();
    final result = await _mapNativeErrors(
      () => gen_groups.leaveGroup(client: _client._inner, groupDid: groupDid),
    );
    return result._toModel();
  }

  Future<String?> getJoinCode(String groupDid) async {
    _client._ensureNotDisposed();
    return _mapNativeErrors(
      () => gen_groups.getGroupJoinCode(
        client: _client._inner,
        groupDid: groupDid,
      ),
    );
  }

  Future<String?> refreshJoinCode(String groupDid) async {
    _client._ensureNotDisposed();
    return _mapNativeErrors(
      () => gen_groups.refreshGroupJoinCode(
        client: _client._inner,
        groupDid: groupDid,
      ),
    );
  }
}

class RealtimeApi {
  RealtimeApi._(this._client);

  final AwikiImClient _client;

  Future<RealtimeCapability> capability() async {
    _client._ensureNotDisposed();
    final capability = await _mapNativeErrors(
      () => gen_realtime.realtimeCapability(client: _client._inner),
    );
    return capability._toModel();
  }

  Future<RealtimeStatus> status() async {
    _client._ensureNotDisposed();
    final status = await _mapNativeErrors(
      () => gen_realtime.realtimeStatus(client: _client._inner),
    );
    return status._toModel();
  }

  Future<void> connect() async {
    await start();
  }

  Future<RealtimeSession> start({
    RealtimeOptions options = const RealtimeOptions(),
  }) async {
    _client._ensureNotDisposed();
    await stop();
    final session = await _mapNativeErrors(
      () => gen_realtime.realtimeStart(
        client: _client._inner,
        options: options._toGen(),
      ),
    );
    _client._realtimeSession = session;
    _client._realtimeEventSubscription = gen_realtime
        .realtimeEventStream(session: session)
        .map((event) => event._toModel())
        .listen(
          _client._emitRealtimeEvent,
          onError: _client._eventsController.addError,
        );
    return _NativeRealtimeSession._(_client, session);
  }

  Future<void> stop() async {
    _client._ensureNotDisposed();
    final session = _client._realtimeSession;
    _client._realtimeSession = null;
    if (session != null) {
      await _mapNativeErrors(() => gen_realtime.realtimeStop(session: session));
    }
    await _client._realtimeEventSubscription?.cancel();
    _client._realtimeEventSubscription = null;
  }
}

class _NativeRealtimeSession implements RealtimeSession {
  _NativeRealtimeSession._(this._client, this._session);

  final AwikiImClient _client;
  final gen_realtime.ArcDartRealtimeSession _session;
  bool _disposed = false;

  @override
  Future<void> stop() async {
    if (_disposed) return;
    if (!_client._disposed && identical(_client._realtimeSession, _session)) {
      await _client.realtime.stop();
    } else {
      await _mapNativeErrors(
        () => gen_realtime.realtimeStop(session: _session),
      );
    }
    _disposed = true;
  }

  @override
  Future<void> dispose() async {
    await stop();
  }
}

extension on AwikiImClient {
  void _emitRealtimeEvent(RealtimeEvent event) {
    if (!_eventsController.isClosed) {
      _eventsController.add(event);
    }
    if (event.isConnectionState && !_connectionStatesController.isClosed) {
      _connectionStatesController.add(
        RealtimeConnectionState(
          state: event.state ?? 'unknown',
          reason: event.reason,
        ),
      );
    }
  }
}

extension on RealtimeOptions {
  gen_realtime_dto.DartRealtimeOptions _toGen() =>
      gen_realtime_dto.DartRealtimeOptions(
        reconnect: switch (reconnect) {
          RealtimeReconnectMode.disabled => 'disabled',
          RealtimeReconnectMode.fixed => 'fixed',
          RealtimeReconnectMode.exponential => 'exponential',
        },
        eventBuffer: eventBuffer,
        reconnectDelayMs: reconnectDelayMs == null
            ? null
            : BigInt.from(reconnectDelayMs!),
        reconnectBaseDelayMs: reconnectBaseDelayMs == null
            ? null
            : BigInt.from(reconnectBaseDelayMs!),
        reconnectMaxDelayMs: reconnectMaxDelayMs == null
            ? null
            : BigInt.from(reconnectMaxDelayMs!),
        reconnectMaxAttempts: reconnectMaxAttempts,
        subscriptions: subscriptions,
      );
}

extension on gen_realtime_dto.DartRealtimeEvent {
  RealtimeEvent _toModel() => RealtimeEvent(
    kind: kind,
    state: state,
    reason: reason,
    message: message?._toModel(),
    messageId: messageId,
    threadKind: threadKind,
    threadId: threadId,
    updateKind: updateKind,
    group: group,
    notificationId: notificationId,
    title: title,
    body: body,
    source: source,
    hostKind: hostKind,
    contentType: contentType,
    notificationType: notificationType,
  );
}

extension on AwikiImCoreConfig {
  gen_config.DartImCoreConfig _toGen() => gen_config.DartImCoreConfig(
    serviceBaseUrl: serviceBaseUrl,
    didDomain: didDomain,
    userServiceEndpoint: userServiceEndpoint,
    messageServiceEndpoint: messageServiceEndpoint,
    mailServiceEndpoint: mailServiceEndpoint,
    anpServiceEndpoint: anpServiceEndpoint,
    anpServiceDid: anpServiceDid,
    transportPolicy: transportPolicy._toGen(),
  );
}

extension on AwikiImCorePaths {
  gen_config.DartImCorePaths _toGen() => gen_config.DartImCorePaths(
    identityRootDir: identityRootDir,
    registryPath: registryPath,
    defaultIdentityPath: defaultIdentityPath,
    sqlitePath: sqlitePath,
    cacheDir: cacheDir,
    tempDir: tempDir,
  );
}

extension on MessageTransportPolicy {
  gen_config.DartMessageTransportPolicy _toGen() => switch (this) {
    MessageTransportPolicy.auto => gen_config.DartMessageTransportPolicy.auto,
    MessageTransportPolicy.httpOnly =>
      gen_config.DartMessageTransportPolicy.httpOnly,
    MessageTransportPolicy.realtimePreferred =>
      gen_config.DartMessageTransportPolicy.realtimePreferred,
  };
}

extension on IdentitySelector {
  gen_identity.DartIdentitySelector _toGen() => switch (this) {
    DefaultIdentitySelector() =>
      const gen_identity.DartIdentitySelector.default_(),
    IdIdentitySelector(:final id) => gen_identity.DartIdentitySelector.id(
      id: id,
    ),
    DidIdentitySelector(:final did) => gen_identity.DartIdentitySelector.did(
      did: did,
    ),
    HandleIdentitySelector(:final handle) =>
      gen_identity.DartIdentitySelector.handle(handle: handle),
    LocalAliasIdentitySelector(:final alias) =>
      gen_identity.DartIdentitySelector.localAlias(alias: alias),
  };
}

extension on gen_identity.DartIdentitySummary {
  IdentitySummary _toModel() => IdentitySummary(
    id: id,
    did: did,
    handle: handle,
    displayName: displayName,
    localAlias: localAlias,
    deviceId: deviceId,
    isDefault: isDefault,
    readyForAuth: readyForAuth,
    readyForMessaging: readyForMessaging,
    missing: missing,
  );
}

extension on InitialProfile {
  gen_identity.DartInitialProfile _toGen() => gen_identity.DartInitialProfile(
    displayName: displayName,
    avatarUrl: avatarUrl,
  );
}

extension on gen_identity.DartDefaultIdentityChange {
  DefaultIdentityChange _toModel() => DefaultIdentityChange(
    previous: previous?._toModel(),
    next: next._toModel(),
    requiresDefaultIdentityWrite: requiresDefaultIdentityWrite,
    warnings: warnings,
  );
}

extension on gen_identity.DartHandleRegistrationResult {
  HandleRegistrationResult _toModel() => HandleRegistrationResult(
    identity: identity?._toModel(),
    handle: handle,
    method: method,
    state: state,
    defaultIdentityChange: defaultIdentityChange?._toModel(),
    warnings: warnings,
  );
}

extension on gen_identity.DartRecoverHandleResult {
  RecoverHandleResult _toModel() => RecoverHandleResult(
    handle: handle,
    phone: phone,
    state: state,
    recoveredIdentity: recoveredIdentity?._toModel(),
    userId: userId,
    accessTokenPresent: accessTokenPresent,
    warnings: warnings,
  );
}

extension on AuthScope {
  gen_auth_dto.DartAuthScope _toGen() => switch (this) {
    AuthScope.userProfile => gen_auth_dto.DartAuthScope.userProfile,
    AuthScope.messaging => gen_auth_dto.DartAuthScope.messaging,
    AuthScope.groupMessaging => gen_auth_dto.DartAuthScope.groupMessaging,
  };
}

extension on gen_auth_dto.DartAuthScope {
  AuthScope _toModel() => switch (this) {
    gen_auth_dto.DartAuthScope.userProfile => AuthScope.userProfile,
    gen_auth_dto.DartAuthScope.messaging => AuthScope.messaging,
    gen_auth_dto.DartAuthScope.groupMessaging => AuthScope.groupMessaging,
  };
}

extension on gen_auth_dto.DartAuthStatus {
  AuthStatus _toModel() => AuthStatus(
    subject: subject,
    hasSession: hasSession,
    expiresAt: expiresAt,
    needsRefresh: needsRefresh,
    warnings: warnings,
  );
}

extension on gen_auth_dto.DartSessionBundle {
  SessionBundle _toModel() => SessionBundle(
    subject: subject,
    scope: scope._toModel(),
    expiresAt: expiresAt,
    refreshed: refreshed,
  );
}

extension on gen_auth_dto.DartSessionUpdate {
  SessionUpdate _toModel() => SessionUpdate(
    subject: subject,
    previousExpiresAt: previousExpiresAt,
    newExpiresAt: newExpiresAt,
    refreshed: refreshed,
  );
}

extension on IdentitySubject {
  gen_directory_dto.DartIdentitySubject _toGen() => switch (this) {
    DidIdentitySubject(:final did) => gen_directory_dto.DartIdentitySubject.did(
      did: did,
    ),
    HandleIdentitySubject(:final handle) =>
      gen_directory_dto.DartIdentitySubject.handle(handle: handle),
    AnyIdentitySubject(:final value) =>
      gen_directory_dto.DartIdentitySubject.any(value: value),
  };
}

extension on gen_directory_dto.DartDirectoryResolution {
  DirectoryResolution _toModel() => DirectoryResolution(
    input: input,
    did: did,
    handle: handle,
    profile: profile?._toModel(),
    warnings: warnings,
  );
}

extension on gen_directory_dto.DartRelationStatus {
  RelationStatus _toModel() => RelationStatus(
    peer: peer,
    relationship: relationship,
    displayName: displayName,
  );
}

extension on ProfilePatch {
  gen_profile_dto.DartProfilePatch _toGen() => gen_profile_dto.DartProfilePatch(
    displayName: displayName,
    bio: bio,
    tags: tags,
    markdown: markdown,
  );
}

extension on gen_profile_dto.DartUserProfile {
  UserProfile _toModel() => UserProfile(
    subject: subject,
    handle: handle,
    displayName: displayName,
    bio: bio,
    tags: tags,
    markdown: markdown,
    avatarUrl: avatarUrl,
    updatedAt: updatedAt,
  );
}

extension on MessageTarget {
  gen_message.DartMessageTarget _toGen() => switch (this) {
    DirectMessageTarget(:final peer) => gen_message.DartMessageTarget.direct(
      peer: peer,
    ),
    GroupMessageTarget(:final group) => gen_message.DartMessageTarget.group(
      group: group,
    ),
  };
}

extension on ThreadRef {
  gen_message.DartThreadRef _toGen() => switch (this) {
    DirectThreadRef(:final peer) => gen_message.DartThreadRef.direct(
      peer: peer,
    ),
    GroupThreadRef(:final group) => gen_message.DartThreadRef.group(
      group: group,
    ),
    MessageThreadRef(:final threadId) => gen_message.DartThreadRef.thread(
      threadId: threadId,
    ),
  };
}

extension on MessageSecurityMode {
  gen_message.DartMessageSecurityMode _toGen() => switch (this) {
    MessageSecurityMode.defaultPlain =>
      gen_message.DartMessageSecurityMode.defaultPlain,
    MessageSecurityMode.plain => gen_message.DartMessageSecurityMode.plain,
    MessageSecurityMode.secureDirect =>
      gen_message.DartMessageSecurityMode.secureDirect,
    MessageSecurityMode.groupE2ee =>
      gen_message.DartMessageSecurityMode.groupE2Ee,
  };
}

extension on SendTextRequest {
  gen_message.DartSendTextRequest _toGen() => gen_message.DartSendTextRequest(
    target: target._toGen(),
    text: text,
    markdown: markdown,
    security: security._toGen(),
    clientMessageId: clientMessageId,
    idempotencyKey: idempotencyKey,
    waitForFinalAcceptance: waitForFinalAcceptance,
  );
}

extension on AttachmentInput {
  gen_attachment.DartAttachmentInput _toGen() => switch (this) {
    LocalFileAttachmentInput(:final path) =>
      gen_attachment.DartAttachmentInput.localFile(path: path),
    BytesAttachmentInput(:final filename, :final mimeType, :final bytes) =>
      gen_attachment.DartAttachmentInput.bytes(
        filename: filename,
        mimeType: mimeType,
        bytes: Uint8List.fromList(bytes),
      ),
  };
}

extension on AttachmentSendRequest {
  gen_attachment.DartAttachmentSendRequest _toGen() =>
      gen_attachment.DartAttachmentSendRequest(
        target: target._toGen(),
        input: input._toGen(),
        caption: caption,
        mimeType: mimeType,
        filename: filename,
        idempotencyKey: idempotencyKey,
        waitForFinalAcceptance: waitForFinalAcceptance,
      );
}

extension on AttachmentDestination {
  gen_attachment.DartAttachmentDestination _toGen() => switch (this) {
    LocalFileAttachmentDestination(:final path) =>
      gen_attachment.DartAttachmentDestination.localFile(path: path),
    MemoryAttachmentDestination() =>
      const gen_attachment.DartAttachmentDestination.memory(),
  };
}

extension on DownloadAttachmentRequest {
  gen_attachment.DartDownloadAttachmentRequest _toGen() =>
      gen_attachment.DartDownloadAttachmentRequest(
        thread: thread._toGen(),
        messageId: messageId,
        attachmentId: attachmentId,
        destination: destination._toGen(),
        overwrite: overwrite,
      );
}

extension on gen_attachment.DartDownloadedAttachmentDestination {
  DownloadedAttachmentDestination _toModel() => switch (this) {
    gen_attachment.DartDownloadedAttachmentDestination_LocalFile(:final path) =>
      DownloadedAttachmentLocalFile(path),
    gen_attachment.DartDownloadedAttachmentDestination_Memory(:final bytes) =>
      DownloadedAttachmentMemory(bytes),
  };
}

extension on gen_attachment.DartDownloadedAttachment {
  DownloadedAttachment _toModel() => DownloadedAttachment(
    attachmentId: attachmentId,
    filename: filename,
    mimeType: mimeType,
    sizeBytes: sizeBytes?.toInt(),
    destination: destination._toModel(),
    warnings: warnings,
  );
}

extension on gen_email_dto.DartEmailAttribute {
  EmailAttribute _toModel() => EmailAttribute(key: key, value: value);
}

extension on gen_email_dto.DartEmailAccount {
  EmailAccount _toModel() => EmailAccount(
    mailboxAddress: mailboxAddress,
    displayName: displayName,
    status: status,
    attributes: attributes.map((attribute) => attribute._toModel()).toList(),
  );
}

extension on gen_email_dto.DartEmailMessageSummary {
  EmailMessageSummary _toModel() => EmailMessageSummary(
    id: id,
    folder: folder,
    from: from,
    to: to,
    cc: cc,
    subject: subject,
    preview: preview,
    receivedAt: receivedAt,
    sentAt: sentAt,
    unread: unread,
    hasAttachments: hasAttachments,
    attachmentCount: attachmentCount,
    attributes: attributes.map((attribute) => attribute._toModel()).toList(),
  );
}

extension on gen_email_dto.DartEmailMessageSummaryPage {
  EmailMessageSummaryPage _toModel() => EmailMessageSummaryPage(
    items: items.map((message) => message._toModel()).toList(),
    nextCursor: nextCursor,
    hasMore: hasMore,
  );
}

extension on gen_email_dto.DartEmailAttachmentMetadata {
  EmailAttachmentMetadata _toModel() => EmailAttachmentMetadata(
    index: index,
    filename: filename,
    contentType: contentType,
    size: size?.toInt(),
  );
}

extension on gen_email_dto.DartEmailMessage {
  EmailMessage _toModel() => EmailMessage(
    summary: summary._toModel(),
    bodyText: bodyText,
    bodyHtml: bodyHtml,
    attachments: attachments
        .map((attachment) => attachment._toModel())
        .toList(),
  );
}

extension on gen_email_dto.DartEmailMarkReadResult {
  EmailMarkReadResult _toModel() => EmailMarkReadResult(updated: updated);
}

extension on SendEmailRequest {
  gen_email_dto.DartSendEmailRequest _toGen() =>
      gen_email_dto.DartSendEmailRequest(
        to: to,
        cc: cc,
        subject: subject,
        bodyText: bodyText,
        bodyHtml: bodyHtml,
      );
}

extension on gen_email_dto.DartSendEmailResult {
  SendEmailResult _toModel() => SendEmailResult(
    accepted: accepted,
    messageId: messageId,
    warnings: warnings,
  );
}

extension on gen_email_dto.DartEmailAttachmentContent {
  EmailAttachmentContent _toModel() => EmailAttachmentContent(
    messageId: messageId,
    attachmentIndex: attachmentIndex,
    filename: filename,
    contentType: contentType,
    size: size?.toInt(),
    bytes: bytes,
  );
}

extension on gen_email_dto.DartEmailNotification {
  EmailNotification _toModel() => EmailNotification(
    id: id,
    mailboxAddress: mailboxAddress,
    fromAddr: fromAddr,
    subject: subject,
    preview: preview,
    hasAttachments: hasAttachments,
    receivedAt: receivedAt,
    attributes: attributes.map((attribute) => attribute._toModel()).toList(),
  );
}

extension on gen_email_dto.DartEmailNotificationPage {
  EmailNotificationPage _toModel() => EmailNotificationPage(
    items: items.map((notification) => notification._toModel()).toList(),
    nextCursor: nextCursor,
    hasMore: hasMore,
  );
}

extension on gen_message.DartMessageDirection {
  MessageDirection _toModel() => switch (this) {
    gen_message.DartMessageDirection.outgoing => MessageDirection.outgoing,
    gen_message.DartMessageDirection.incoming => MessageDirection.incoming,
    gen_message.DartMessageDirection.unknown => MessageDirection.unknown,
  };
}

extension on gen_message.DartMessageBodyView {
  MessageBodyView _toModel() => MessageBodyView(
    text: text,
    kind: kind,
    unsupportedContentType: unsupportedContentType,
  );
}

extension on gen_message.DartMessageMetadataAttribute {
  MessageMetadataAttribute _toModel() =>
      MessageMetadataAttribute(key: key, value: value);
}

extension on gen_message.DartMessageMetadata {
  MessageMetadata _toModel() => MessageMetadata(
    operationId: operationId,
    deliveryState: deliveryState,
    sendState: sendState,
    retryable: retryable,
    retryAction: retryAction,
    serverSequence: serverSequence,
    contentType: contentType,
    attributes: attributes.map((attribute) => attribute._toModel()).toList(),
  );
}

extension on gen_message.DartMessage {
  Message _toModel() => Message(
    id: id,
    threadKind: threadKind,
    threadId: threadId,
    direction: direction._toModel(),
    sender: sender,
    receiver: receiver,
    group: group,
    body: body._toModel(),
    sentAt: sentAt,
    receivedAt: receivedAt,
    metadata: metadata._toModel(),
  );
}

extension on gen_message.DartMessagePage {
  MessagePage _toModel() => MessagePage(
    items: items.map((message) => message._toModel()).toList(),
    nextCursor: nextCursor,
    hasMore: hasMore,
  );
}

extension on gen_message.DartConversation {
  Conversation _toModel() => Conversation(
    threadKind: threadKind,
    threadId: threadId,
    title: title,
    participants: participants,
    lastMessage: lastMessage?._toModel(),
    unreadCount: unreadCount,
    messageCount: messageCount,
    lastMessageAt: lastMessageAt,
  );
}

extension on gen_message.DartConversationPage {
  ConversationPage _toModel() => ConversationPage(
    items: items.map((conversation) => conversation._toModel()).toList(),
    nextCursor: nextCursor,
    hasMore: hasMore,
  );
}

extension on gen_message.DartSendMessageResult {
  SendMessageResult _toModel() => SendMessageResult(
    message: message._toModel(),
    deliveryState: deliveryState,
    warnings: warnings,
  );
}

extension on gen_message.DartMarkReadResult {
  MarkReadResult _toModel() => MarkReadResult(
    updatedCount: updatedCount,
    messageIds: messageIds,
    warnings: warnings,
  );
}

extension on CreateGroupRequest {
  gen_group_dto.DartCreateGroupRequest _toGen() =>
      gen_group_dto.DartCreateGroupRequest(
        name: name,
        description: description,
        discoverability: discoverability?.value,
        admissionMode: admissionMode?.value,
        messageSecurityProfile: messageSecurityProfile?.value,
        e2Ee: e2ee,
        slug: slug,
        goal: goal,
        rules: rules,
        messagePrompt: messagePrompt,
        docUrl: docUrl,
        attachmentsAllowed: attachmentsAllowed,
        maxMembers: maxMembers?.value.toString(),
        memberMaxMessages: memberMaxMessages,
        memberMaxTotalChars: memberMaxTotalChars,
      );
}

extension on gen_group_dto.DartGroupSummary {
  GroupSummary _toModel() => GroupSummary(
    id: id,
    did: did,
    name: name,
    membershipStatus: membershipStatus,
    memberCount: memberCount,
    lastMessageAt: lastMessageAt,
  );
}

extension on gen_group_dto.DartGroupSnapshot {
  GroupSnapshot _toModel() => GroupSnapshot(
    id: id,
    did: did,
    name: name,
    description: description,
    myRole: myRole,
    membershipStatus: membershipStatus,
    memberCount: memberCount,
    lastMessageAt: lastMessageAt,
  );
}

extension on gen_group_dto.DartGroupMember {
  GroupMember _toModel() => GroupMember(
    did: did,
    handle: handle,
    role: role,
    status: status,
    joinedAt: joinedAt,
  );
}

extension on gen_group_dto.DartGroupReadResult {
  GroupReadResult _toModel() => GroupReadResult(
    group: group?._toModel(),
    groups: groups.map((group) => group._toModel()).toList(),
    members: members.map((member) => member._toModel()).toList(),
    messages: messages._toModel(),
    total: total,
    source: source,
    warnings: warnings,
  );
}

extension on gen_realtime_dto.DartRealtimeCapability {
  RealtimeCapability _toModel() => RealtimeCapability(
    statusSupported: statusSupported,
    connectSupported: connectSupported,
    runnerExposed: runnerExposed,
    reason: reason,
  );
}

extension on gen_realtime_dto.DartRealtimeStatus {
  RealtimeStatus _toModel() => RealtimeStatus(
    connected: connected,
    state: state,
    subscriptions: subscriptions,
    lastError: lastError,
    warnings: warnings,
  );
}
