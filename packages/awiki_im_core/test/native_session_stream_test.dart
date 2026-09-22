import 'dart:async';
import 'package:awiki_im_core/src/native_session_stream.dart';
import 'package:test/test.dart';

// Models the bridge reader: cancellation while idle only completes once the
// native producer wakes or closes the receive port.
Stream<int> bridge(Stream<int> source) async* {
  await for (final patch in source) {
    yield patch;
  }
}

void main() {
  test(
    'idle patch cancellation stops producer before waiting on bridge',
    () async {
      for (var generation = 1; generation <= 2; generation++) {
        final source = StreamController<int>();
        var stops = 0;
        final stream = nativeSessionStream<int, int>(
          open: () async => generation,
          events: (_) => bridge(source.stream),
          stop: (session) async {
            expect(session, generation);
            stops++;
            await source.close();
          },
        );
        final ready = Completer<int>();
        final subscription = stream.listen(ready.complete);
        source.add(generation);
        expect(await ready.future, generation);
        await Future<void>.delayed(Duration.zero);
        await subscription.cancel().timeout(const Duration(seconds: 1));
        expect(stops, 1);
        expect(source.hasListener, isFalse);
      }
    },
  );

  test(
    'cancel during open releases session without attaching reader',
    () async {
      final opening = Completer<int>();
      var attached = false;
      final stopped = <int>[];
      final stream = nativeSessionStream<int, int>(
        open: () => opening.future,
        events: (_) {
          attached = true;
          return const Stream.empty();
        },
        stop: (session) async {
          stopped.add(session);
        },
      );
      final sub = stream.listen((_) => fail('cancelled stream emitted'));
      final cancellation = sub.cancel();
      opening.complete(42);
      await cancellation.timeout(const Duration(seconds: 1));
      expect(attached, isFalse);
      expect(stopped, [42]);
    },
  );

  test('natural stream completion releases the session exactly once', () async {
    var stops = 0;
    final stream = nativeSessionStream<int, int>(
      open: () async => 1,
      events: (_) => Stream.fromIterable([1, 2]),
      stop: (_) async {
        stops++;
      },
    );
    expect(await stream.toList().timeout(const Duration(seconds: 1)), [1, 2]);
    expect(stops, 1);
  });

  test(
    'failed session open preserves error and does not stop absent resource',
    () async {
      var stops = 0;
      final error = StateError('open failed');
      final stream = nativeSessionStream<int, int>(
        open: () async => throw error,
        events: (_) => const Stream.empty(),
        stop: (_) async {
          stops++;
        },
      );
      await expectLater(
        stream,
        emitsInOrder([emitsError(same(error)), emitsDone]),
      );
      expect(stops, 0);
    },
  );
}
