import 'dart:io';

Map<String, String> osEnvironment() => Platform.environment;

String? readFileUtf8(String path) {
  try {
    return File(path).readAsStringSync();
  } catch (_) {
    return null;
  }
}
