import 'models/config.dart';
import 'models/identity.dart';
import 'models/message.dart';
import 'models/secure.dart';

UnsupportedError _unsupported() => UnsupportedError(
  'awiki_im_core native Rust backend is not supported on Flutter Web.',
);

class AwikiImCore {
  static Future<AwikiImCore> open({
    required AwikiImCoreConfig config,
    required AwikiImCorePaths paths,
  }) async {
    throw _unsupported();
  }

  Future<List<IdentitySummary>> listIdentities() async {
    throw _unsupported();
  }

  Future<IdentitySummary?> defaultIdentity() async {
    throw _unsupported();
  }

  Future<IdentitySummary> resolveIdentity(IdentitySelector selector) async {
    throw _unsupported();
  }

  Future<DeleteLocalIdentityResult> deleteLocalIdentity(
    IdentitySelector selector,
  ) async {
    throw _unsupported();
  }

  Future<DaemonSubkeyPrivatePackage> loadDaemonSubkeyPackage(
    IdentitySelector selector,
  ) async {
    throw _unsupported();
  }

  Future<DaemonSubkeyPrivatePackage> ensureDaemonSubkeyPackage(
    IdentitySelector selector,
  ) async {
    throw _unsupported();
  }

  Future<AwikiImClient> client(IdentitySelector selector) async {
    throw _unsupported();
  }

  Future<void> dispose() async {}
}

class AwikiImClient {
  MessageApi get messages => MessageApi._();

  SecureApi get secure => SecureApi._();

  Future<void> dispose() async {}
}

class MessageApi {
  MessageApi._();

  Future<MessagePage> localHistory(
    ThreadRef thread, {
    required int limit,
    String? cursor,
  }) async {
    throw _unsupported();
  }

  Future<MarkThreadReadResult> markThreadRead(
    ThreadRef thread, {
    int? maxMessageIds,
  }) async {
    throw _unsupported();
  }

  Future<SyncDeltaResult> syncDelta(SyncDeltaRequest request) async {
    throw _unsupported();
  }

  Future<SyncThreadAfterResult> syncThreadAfter(
    SyncThreadAfterRequest request,
  ) async {
    throw _unsupported();
  }

  Future<ConversationListSnapshot?> loadConversationSnapshot() async {
    throw _unsupported();
  }

  Future<void> clearConversationSnapshot() async {
    throw _unsupported();
  }
}

class SecureApi {
  SecureApi._();

  DirectSecureConversation direct(String peer) => DirectSecureConversation._();

  GroupSecureConversation group(String group) => GroupSecureConversation._();

  SecureOutboxApi get outbox => SecureOutboxApi._();
}

class DirectSecureConversation {
  DirectSecureConversation._();

  Future<DirectSecureStatus> status() async {
    throw _unsupported();
  }

  Future<DirectSecurePrepareResult> prepare() async {
    throw _unsupported();
  }

  Future<DirectSecureRepairResult> repair() async {
    throw _unsupported();
  }
}

class GroupSecureConversation {
  GroupSecureConversation._();

  Future<GroupSecureStatus> status() async {
    throw _unsupported();
  }

  Future<GroupSecurePrepareResult> prepare() async {
    throw _unsupported();
  }

  Future<GroupSecureRepairResult> repair() async {
    throw _unsupported();
  }
}

class SecureOutboxApi {
  SecureOutboxApi._();

  Future<List<SecureOutboxEntry>> listFailed() async {
    throw _unsupported();
  }

  Future<SecureOutboxResult> retry(String outboxId) async {
    throw _unsupported();
  }

  Future<SecureOutboxResult> drop(String outboxId) async {
    throw _unsupported();
  }
}
