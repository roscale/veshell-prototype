import 'dart:io';

import 'package:args/command_runner.dart';
import 'package:mason_logger/mason_logger.dart';

import '../util.dart';
import '../veshell.dart';
import 'dependency/check_depencies.dart' as dependencies;

const targetExec = 'veshell';

class BuildCommand extends Command<int> {
  BuildCommand({required this.logger});

  final Logger logger;
  @override
  final name = 'build';
  @override
  final description = 'Build and package Veshell';

  // [run] may also return a Future.
  @override
  Future<int> run() async {
    final target =
        BuildTarget.values.byName(globalResults?['target'] as String);
    await dependencies.check(logger);
    await buildAll(logger, target: target);
    return ExitCode.success.code;
  }
}

Future<void> buildAll(
  Logger logger, {
  required BuildTarget target,
}) async {
  await buildEmbedder(logger, target: target);
  await buildShell(logger, target: target);
  await packageBuild(logger, target: target);
}

Future<void> buildEmbedder(
  Logger logger, {
  required BuildTarget target,
}) async {
  logger.info('Building the rust embedder in ${target.name}...\n');

  final cargoArgs = ['build', '--color', 'always'];
  if (target != BuildTarget.debug) {
    // Profile mode does not exist in Cargo by default.
    // Just compile in release mode instead.
    cargoArgs.add('--release');
  }

  final exitCode = await runProcess(
    'cargo',
    cargoArgs,
    workingDirectory: 'embedder',
    environment: {
      'BUNDLE': 'true',
      'FLUTTER_ENGINE_BUILD': target.name,
      'RUST_LOG': logger.level == Level.verbose ? 'debug' : 'warn'
    },
  );
  if (exitCode != 0) {
    exit(exitCode);
  }
  logger.success('Build completed\n');
}

Future<void> buildShell(
  Logger logger, {
  required BuildTarget target,
}) async {
  logger.info('Building the shell in ${target.name}...\n');
  var exitCode = await (await Process.start(
    'dart',
    [
      'run',
      'build_runner',
      'build',
      '--delete-conflicting-outputs',
    ],
    workingDirectory: 'shell',
    mode: ProcessStartMode.inheritStdio,
  ))
      .exitCode;

  if (exitCode != 0) {
    exit(exitCode);
  }

  exitCode = await (await Process.start(
    'flutter',
    [
      'build',
      'linux',
      '--${target.name}',
    ],
    workingDirectory: 'shell',
    mode: ProcessStartMode.inheritStdio,
  ))
      .exitCode;
  if (exitCode != 0) {
    exit(exitCode);
  }
  logger.success('\nBuild completed\n');
}

Future<void> packageBuild(
  Logger logger, {
  required BuildTarget target,
}) async {
  logger.info('Packaging build...\n');

  final unameM = (await Process.run('uname', ['-m'])).stdout as String;

  var arch = '';

  if (unameM.contains('x86_64') == true) {
    arch = 'x64';
  } else {
    arch = 'arm64';
  }

  final buildDirectory = 'build/${target.name}';
  logger.info('Create $buildDirectory directory');
  await Directory('$buildDirectory/lib/').create(recursive: true);

  var cargoTarget = target;
  if (cargoTarget == BuildTarget.profile) {
    cargoTarget = BuildTarget.release;
  }

  final binaryPath = 'embedder/target/${cargoTarget.name}/$targetExec';
  logger.info('Copy $binaryPath');

  final targetPath = '$buildDirectory/$targetExec';

  // handle (OS Error: Text file busy, errno = 26)
  try {
    await File(binaryPath).copy(targetPath);
  } on FileSystemException catch (e) {
    if (e.osError?.errorCode == 26) {
      final result = Process.runSync('lsof', ['-t', binaryPath]);
      logger.info('kill running process ${result.stdout}');
      final pid = int.tryParse((result.stdout as String).trim());
      if (pid != null) {
        Process.runSync('kill', ['-9', pid.toString()]);
        await File(binaryPath).copy(targetPath); // Retry the copy operation
      }
    } else {
      rethrow; // If it's another error, rethrow it
    }
  }

  final libFlutterEnginePath =
      'embedder/third_party/flutter_engine/${target.name}/libflutter_engine.so';

  logger.info('Copy $libFlutterEnginePath');
  await File(libFlutterEnginePath)
      .copy('$buildDirectory/lib/libflutter_engine.so');

  if (target != BuildTarget.debug) {
    final libAppPath =
        'shell/build/linux/$arch/${target.name}/bundle/lib/libapp.so';
    logger.info('Copy $libAppPath');
    await File(libAppPath).copy('$buildDirectory/lib/libapp.so');
  }

  await runProcess(
    'cp',
    [
      '-r',
      'shell/build/linux/$arch/${target.name}/bundle/data',
      buildDirectory,
    ],
  );

  logger.success('\nPackaging completed\n');
}
