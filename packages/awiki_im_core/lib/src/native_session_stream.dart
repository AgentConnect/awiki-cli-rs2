import 'dart:async';

/// Stops the producer before awaiting cancellation of an idle bridge stream.
///
/// The bridge's async generator may be awaiting a native event. Cancelling it
/// first would wait for that event before the producer's stop can run.
Stream<T> nativeSessionStream<S, T>({
  required Future<S> Function() open,
  required Stream<T> Function(S session) events,
  required Future<void> Function(S session) stop,
}) {
  late final StreamController<T> controller;
  Future<S>? opening;
  StreamSubscription<T>? subscription;
  Future<void>? stopping;
  var cancelled = false;

  Future<void> cancel() {
    cancelled = true;
    return stopping ??= () async {
      final S session;
      try {
        session = await opening!;
      } catch (_) {
        // Opening failed; there is no native resource to release.
        return;
      }
      try {
        await stop(session);
      } finally {
        await subscription?.cancel();
      }
    }();
  }

  Future<void> start() async {
    opening = Future<S>.sync(open);
    try {
      final session = await opening!;
      if (cancelled) return;
      subscription = events(session).listen(
        controller.add,
        onError: controller.addError,
        onDone: () => unawaited(controller.close()),
      );
      if (controller.isPaused) subscription!.pause();
    } catch (error, stack) {
      if (!cancelled) {
        controller.addError(error, stack);
        unawaited(controller.close());
      }
    }
  }

  controller = StreamController<T>(
    onListen: () => unawaited(start()),
    onPause: () => subscription?.pause(),
    onResume: () => subscription?.resume(),
    onCancel: cancel,
  );
  return controller.stream;
}
