import 'package:flutter/material.dart';
import 'package:flutter/scheduler.dart';
import 'package:hooks_riverpod/hooks_riverpod.dart';
import 'package:shell/display/widget/display.dart';
import 'package:shell/screen/provider/screen_list.dart';
import 'package:shell/shared/provider/root_overlay.dart';
import 'package:shell/theme/provider/theme.manager.dart';
import 'package:shell/wayland/provider/surface.manager.dart';
import 'package:shell/wayland/provider/wayland.manager.dart';
import 'package:shell/window/provider/window.manager.dart';
import 'package:visibility_detector/visibility_detector.dart';

void main() {
  // debugRepaintRainbowEnabled = true;
  // debugPrintGestureArenaDiagnostics = true;
  WidgetsFlutterBinding.ensureInitialized();
  final container = ProviderContainer();

  SchedulerBinding.instance.addPostFrameCallback((_) {
    //platformApi.startupComplete();
  });

  container
    ..read(waylandManagerProvider)
    ..read(surfaceManagerProvider)
    ..read(screenListProvider.notifier).createNewScreen()
    ..read(windowManagerProvider);

  VisibilityDetectorController.instance.updateInterval = Duration.zero;

  runApp(
    UncontrolledProviderScope(
      container: container,
      child: const Veshell(),
    ),
  );
}

class Veshell extends ConsumerWidget {
  const Veshell({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final theme = ref.watch(themeProvider);
    return MaterialApp(
      theme: theme,
      home: Scaffold(
        body: Consumer(
          builder: (
            BuildContext context,
            WidgetRef ref,
            Widget? child,
          ) {
            return Stack(
              children: [
                const DisplayWidget(),
                Overlay(
                  key: ref.watch(rootOverlayKeyProvider),
                ),
              ],
            );
          },
        ),
      ),
    );
  }
}
