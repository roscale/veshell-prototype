import 'package:flutter/material.dart';
import 'package:flutter_hooks/flutter_hooks.dart';
import 'package:hooks_riverpod/hooks_riverpod.dart';
import 'package:material_design_icons_flutter/material_design_icons_flutter.dart';
import 'package:shell/application/widget/app_icon.dart';
import 'package:shell/wayland/model/request/close_window/close_window.serializable.dart';
import 'package:shell/wayland/model/request/resize_window/resize_window.serializable.dart';
import 'package:shell/wayland/provider/wayland.manager.dart';
import 'package:shell/wayland/widget/xdg_toplevel_surface.dart';
import 'package:shell/window/model/window.dart';
import 'package:shell/window/provider/dialog_list_for_window.dart';
import 'package:shell/window/provider/window.manager.dart';
import 'package:shell/window/provider/window_state.dart';
import 'package:shell/workspace/widget/tileable/persistent_window/window_placeholder.dart';
import 'package:shell/workspace/widget/tileable/tileable.dart';

/// Tileable Window that persist when closed
class PersistentWindowTileable extends Tileable {
  /// Const constructor
  const PersistentWindowTileable({required this.windowId, super.key});

  /// The id of the wayland surface
  final WindowId windowId;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final window = ref.watch(windowStateProvider(windowId)) as PersistentWindow;
    final dialogWindowIdSet = ref.watch(
      dialogListForWindowProvider.select((value) => value.get(windowId)),
    );
    final dialogWindowList = <DialogWindow>[];
    for (final windowId in dialogWindowIdSet) {
      final dialogWindow =
          ref.read(windowStateProvider(windowId)) as DialogWindow;
      dialogWindowList.add(dialogWindow);
    }
    if (window.surfaceId != null) {
      return LayoutBuilder(
        builder: (context, constraints) {
          return HookBuilder(
            builder: (context) {
              useEffect(
                () {
                  ref.read(waylandManagerProvider.notifier).request(
                        ResizeWindowRequest(
                          message: ResizeWindowMessage(
                            surfaceId: window.surfaceId!,
                            width:
                                constraints.widthConstraints().maxWidth.round(),
                            height: constraints
                                .heightConstraints()
                                .maxHeight
                                .round(),
                          ),
                        ),
                      );
                  return null;
                },
                [constraints],
              );
              return Stack(
                fit: StackFit.expand,
                children: [
                  XdgToplevelSurfaceWidget(
                    surfaceId: window.surfaceId!,
                  ),
                  if (dialogWindowList.isNotEmpty)
                    for (final dialog in dialogWindowList)
                      XdgToplevelSurfaceWidget(
                        surfaceId: dialog.surfaceId,
                      ),
                ],
              );
            },
          );
        },
      );
    } else {
      return WindowPlaceholder(
        windowId: windowId,
      );
    }
  }

  @override
  Widget buildPanelWidget(BuildContext context, WidgetRef ref) {
    final window = ref.watch(windowStateProvider(windowId)) as PersistentWindow;

    final title = window.title;
    return Tab(
      child: Row(
        children: [
          SizedBox(height: 24, width: 24, child: AppIconById(id: window.appId)),
          const SizedBox(
            width: 16,
          ),
          Text(title),
          const SizedBox(
            width: 16,
          ),
          IconButton(
            visualDensity: VisualDensity.compact,
            iconSize: 16,
            onPressed: () {
              ref.read(waylandManagerProvider.notifier).request(
                    CloseWindowRequest(
                      message: CloseWindowMessage(
                        surfaceId: window.surfaceId!,
                      ),
                    ),
                  );
            },
            icon: Icon(MdiIcons.close),
          ),
        ],
      ),
    );
  }
}