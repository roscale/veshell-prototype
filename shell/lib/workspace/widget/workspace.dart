import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_hooks/flutter_hooks.dart';
import 'package:hooks_riverpod/hooks_riverpod.dart';
import 'package:shell/shared/provider/app_launch.dart';
import 'package:shell/shared/widget/sliding_container.dart';
import 'package:shell/window/provider/persistant_window_state.dart';
import 'package:shell/window/provider/window.manager.dart';
import 'package:shell/workspace/model/workspace_shortcuts.dart';
import 'package:shell/workspace/provider/current_workspace_id.dart';
import 'package:shell/workspace/provider/workspace_state.dart';
import 'package:shell/workspace/widget/tileable/persistent_application_launcher/persistent_application_launcher.dart';
import 'package:shell/workspace/widget/tileable/persistent_window/persistent_window.dart';
import 'package:shell/workspace/widget/tileable/tileable.dart';
import 'package:shell/workspace/widget/workspace_panel.dart';

class WorkspaceWidget extends HookConsumerWidget {
  const WorkspaceWidget({required this.isSelected, super.key});

  final bool isSelected;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final workspaceFocusScopeNode = useFocusScopeNode(
      debugLabel: 'WorkspaceScope',
      canRequestFocus: isSelected,
    );

    useEffect(
      () {
        if (isSelected) {
          workspaceFocusScopeNode
            ..canRequestFocus = isSelected
            ..requestFocus();
        }
        return null;
      },
      [isSelected],
    );
    final windowManager = ref.watch(windowManagerProvider.notifier);
    final currentWorkspaceId = ref.watch(currentWorkspaceIdProvider);
    final workspaceState =
        ref.watch(workspaceStateProvider(currentWorkspaceId));

    final appLauncher = PersistentApplicationSelector(
      isFocused: workspaceState.focusedIndex ==
          workspaceState.tileableWindowList.length,
      onSelect: (entry) {
        print('start ${entry.desktopEntry.id}');
        final newWindowId =
            windowManager.createPersistentWindowForDesktopEntry(entry);
        ref
            .read(workspaceStateProvider(currentWorkspaceId).notifier)
            .addWindow(newWindowId);

        ref
            .read(appLaunchProvider.notifier)
            .launchDesktopEntry(entry.desktopEntry);
      },
    );

    final tileableList = <Tileable>[];
    for (final (index, windowId) in workspaceState.tileableWindowList.indexed) {
      tileableList.add(
        PersistentWindowTileable(
          windowId: windowId,
          isFocused: workspaceState.focusedIndex == index,
        ),
      );
    }
    tileableList.add(appLauncher);
    return Shortcuts(
      shortcuts: <LogicalKeySet, Intent>{
        LogicalKeySet(LogicalKeyboardKey.meta, LogicalKeyboardKey.keyA):
            const FocusLeftTileableIntent(),
        LogicalKeySet(LogicalKeyboardKey.meta, LogicalKeyboardKey.keyD):
            const FocusRightTileableIntent(),
        LogicalKeySet(LogicalKeyboardKey.meta, LogicalKeyboardKey.keyQ):
            const CloseTileableIntent(),
      },
      child: Actions(
        actions: {
          FocusLeftTileableIntent: CallbackAction<FocusLeftTileableIntent>(
            onInvoke: (_) {
              final nextIndex = workspaceState.focusedIndex - 1;
              if (nextIndex >= 0) {
                ref
                    .read(workspaceStateProvider(currentWorkspaceId).notifier)
                    .setFocusedIndex(nextIndex);
              }
              return null;
            },
          ),
          FocusRightTileableIntent: CallbackAction<FocusRightTileableIntent>(
            onInvoke: (_) {
              final nextIndex = workspaceState.focusedIndex + 1;
              if (nextIndex < tileableList.length) {
                ref
                    .read(workspaceStateProvider(currentWorkspaceId).notifier)
                    .setFocusedIndex(nextIndex);
              }
              return null;
            },
          ),
          CloseTileableIntent: CallbackAction<CloseTileableIntent>(
            onInvoke: (_) {
              final tileable = tileableList[workspaceState.focusedIndex];
              if (tileable is PersistentWindowTileable) {
                final persistentWindow =
                    ref.read(persistentWindowStateProvider(tileable.windowId));
                ref.read(windowManagerProvider.notifier).closeWindow(
                      persistentWindow.windowId,
                    );
              }

              return null;
            },
          ),
        },
        child: FocusScope(
          node: workspaceFocusScopeNode,
          onFocusChange: (value) {},
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              WorkspacePanel(
                tileableList: tileableList,
                visibleLength: workspaceState.visibleLength,
                onVisibleLengthChange: (value) {
                  ref
                      .read(workspaceStateProvider(currentWorkspaceId).notifier)
                      .setVisibleLength(value);
                },
              ),
              Expanded(
                child: SlidingContainer(
                  index: workspaceState.focusedIndex,
                  visible: workspaceState.visibleLength,
                  children: tileableList,
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}
