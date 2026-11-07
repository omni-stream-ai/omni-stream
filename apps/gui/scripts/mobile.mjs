import { spawnSync } from 'node:child_process';
import { cpSync, existsSync, mkdirSync, readFileSync, readdirSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import { basename, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { ensureIcons } from './icons.mjs';

const gui = fileURLToPath(new URL('../', import.meta.url));
const native = join(gui, 'src-tauri');
const [platform, command, ...args] = process.argv.slice(2);

function tauri(...arguments_) {
  const result = spawnSync(process.platform === 'win32' ? 'pnpm.cmd' : 'pnpm', ['exec', 'tauri', ...arguments_], {
    cwd: gui, stdio: 'inherit', env: process.env, shell: process.platform === 'win32',
  });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`Tauri exited with status ${result.status ?? result.signal}.`);
}

function configureAndroid() {
  const project = join(native, 'gen', 'android');
  // Firebase Messaging 26 uses Kotlin 2.1 metadata; Tauri's template still uses 1.9.
  const rootGradle = join(project, 'build.gradle.kts');
  writeFileSync(rootGradle, readFileSync(rootGradle, 'utf8').replace(/org.jetbrains.kotlin:kotlin-gradle-plugin:[^"]+/, 'org.jetbrains.kotlin:kotlin-gradle-plugin:2.1.21'));
  const manifestPath = join(project, 'app', 'src', 'main', 'AndroidManifest.xml');
  let manifest = readFileSync(manifestPath, 'utf8');
  // Core addresses are user-configured and can be HTTP on a trusted LAN.
  if (!/android:usesCleartextTraffic="[^"]*"/.test(manifest)) throw new Error('Android manifest is missing usesCleartextTraffic. Run android:init again.');
  manifest = manifest.replace(/android:usesCleartextTraffic="[^"]*"/, 'android:usesCleartextTraffic="true"');
  if (!manifest.includes('android:windowSoftInputMode=')) {
    manifest = manifest.replace('<activity', '<activity android:windowSoftInputMode="adjustResize"');
  }
  if (!manifest.includes('OmniMessagingService')) {
    manifest = manifest.replace('</application>',
      '<service android:name="com.omnistreamai.app.push.OmniMessagingService" android:exported="false"><intent-filter><action android:name="com.google.firebase.MESSAGING_EVENT" /></intent-filter></service>\n' +
      '<meta-data android:name="com.google.firebase.messaging.default_notification_channel_id" android:value="omnistream_remote" />\n' +
      '<meta-data android:name="com.google.firebase.messaging.default_notification_icon" android:resource="@drawable/ic_stat_omnistream" />\n' +
      '<meta-data android:name="firebase_messaging_auto_init_enabled" android:value="false" />\n' +
      '<meta-data android:name="firebase_analytics_collection_enabled" android:value="false" />\n</application>');
  }
  if (!manifest.includes('android:allowBackup=')) manifest = manifest.replace('<application', '<application android:allowBackup="false"');
  writeFileSync(manifestPath, manifest);
  cpSync(join(native, 'icons', 'android'), join(project, 'app', 'src', 'main', 'res'), { recursive: true, filter: source => basename(source) !== '.source-hash' });
  rmSync(join(project, 'app', 'src', 'main', 'res', '.source-hash'), { force: true });

  const push = join(gui, 'native', 'android');
  const source = join(project, 'app', 'src', 'main', 'java', 'com', 'omnistreamai', 'app', 'push');
  mkdirSync(source, { recursive: true });
  cpSync(join(push, 'RemotePushPlugin.kt'), join(source, 'RemotePushPlugin.kt'));
  const res = join(project, 'app', 'src', 'main', 'res');
  mkdirSync(join(res, 'drawable'), { recursive: true });
  mkdirSync(join(res, 'values'), { recursive: true });
  cpSync(join(push, 'ic_stat_omnistream.xml'), join(res, 'drawable', 'ic_stat_omnistream.xml'));
  const firebase = JSON.parse(readFileSync(join(push, 'google-services.json'), 'utf8'));
  const client = firebase.client.find(item => item.client_info.android_client_info.package_name === JSON.parse(readFileSync(join(native, 'tauri.conf.json'), 'utf8')).identifier);
  if (!client) throw new Error('Firebase Android configuration does not match the application identifier.');
  const xml = value => String(value).replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('"', '&quot;');
  const resources = { google_app_id: client.client_info.mobilesdk_app_id, gcm_defaultSenderId: firebase.project_info.project_number,
    google_api_key: client.api_key[0].current_key, project_id: firebase.project_info.project_id, google_storage_bucket: firebase.project_info.storage_bucket };
  writeFileSync(join(res, 'values', 'firebase.xml'), '<resources>' + Object.entries(resources).map(([key, value]) => '<string name="' + key + '" translatable="false">' + xml(value) + '</string>').join('') + '</resources>\n');

  const gradlePath = join(project, 'app', 'build.gradle.kts');
  let gradle = readFileSync(gradlePath, 'utf8');
  // Keep Gradle's tools aligned with the SDK/NDK provisioned by CI instead of
  // silently downloading the Android Gradle plugin's default versions.
  for (const [property, variable] of [
    ['buildToolsVersion', 'ANDROID_BUILD_TOOLS_VERSION'],
    ['ndkVersion', 'ANDROID_NDK_VERSION'],
  ]) {
    const value = process.env[variable];
    if (!value) continue;
    if (!/^[0-9]+[.][0-9]+[.][0-9]+$/.test(value)) throw new Error('Invalid ' + variable + ': use a stable three-part SDK version.');
    const setting = '    ' + property + ' = "' + value + '"';
    const existing = new RegExp('^\\s*' + property + '\\s*=.*$', 'm');
    gradle = existing.test(gradle) ? gradle.replace(existing, setting) : gradle.replace('android {', 'android {\n' + setting);
  }
  if (!gradle.includes('com.google.firebase:firebase-messaging:')) {
    gradle = gradle.replace('dependencies {', 'dependencies {\n    implementation("com.google.firebase:firebase-messaging:26.0.1")');
  }
  if (!gradle.includes('// OmniStream release signing')) {
    if (!gradle.includes('android {')) throw new Error('Android Gradle configuration is missing its android block. Run android:init again.');
    gradle = gradle.replace('android {', `android {
    // OmniStream release signing: keep credentials in the ignored native project.
    val keystorePropertiesFile = rootProject.file("keystore.properties")
    if (keystorePropertiesFile.exists()) {
        val keys = Properties().apply { keystorePropertiesFile.inputStream().use { load(it) } }
        val releaseSigning = signingConfigs.create("omnistreamRelease") {
            storeFile = file(keys.getProperty("storeFile") ?: error("Missing storeFile"))
            storePassword = keys.getProperty("storePassword")
            keyAlias = keys.getProperty("keyAlias")
            keyPassword = keys.getProperty("keyPassword")
        }
        buildTypes.getByName("release").signingConfig = releaseSigning
    }
`);
  }
  // Tauri 2.12's generated Rust task requires an explicit working directory.
  if (!gradle.includes('// OmniStream Rust task working directory')) {
    gradle += '\n\n// OmniStream Rust task working directory\ntasks.withType<BuildTask>().configureEach {\n    projectDir = project.projectDir.absolutePath\n}\n';
  }
  writeFileSync(gradlePath, gradle);
}

function configureIosIcons(project) {
  function sync(directory, depth = 0) {
    if (depth > 5) return;
    if (directory.endsWith('AppIcon.appiconset')) {
      const contents = join(directory, 'Contents.json');
      if (!existsSync(contents)) return;
      for (const { filename } of JSON.parse(readFileSync(contents, 'utf8')).images ?? []) {
        if (!filename) continue;
        const source = join(native, 'icons', 'ios', filename);
        if (existsSync(source)) cpSync(source, join(directory, filename));
      }
      return;
    }
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      if (entry.isDirectory() && !['Pods', 'build', 'target', '.git'].includes(entry.name)) sync(join(directory, entry.name), depth + 1);
    }
  }
  sync(project);
}

try {
  if (!['android', 'ios'].includes(platform) || !['init', 'dev', 'build'].includes(command)) {
    throw new Error('Usage: node scripts/mobile.mjs <android|ios> <init|dev|build> [Tauri options]');
  }
  if (platform === 'ios' && process.platform !== 'darwin') throw new Error('iOS development and packaging require macOS and Xcode.');
  if (args.includes('--help') || args.includes('-h')) {
    tauri(platform, command, ...args);
    process.exit(0);
  }
  ensureIcons();
  const project = join(native, 'gen', platform === 'android' ? 'android' : 'apple');
  const identifier = JSON.parse(readFileSync(join(native, 'tauri.conf.json'), 'utf8')).identifier;
  const identityFile = join(project, '.omnistream-identifier');
  let previousIdentifier = existsSync(identityFile) ? readFileSync(identityFile, 'utf8').trim() : undefined;
  const gradlePath = join(project, 'app', 'build.gradle.kts');
  if (!previousIdentifier && platform === 'android' && existsSync(gradlePath)) {
    previousIdentifier = readFileSync(gradlePath, 'utf8').match(/namespace\s*=\s*"([^"]+)"/)?.[1];
  }
  let previousSigning;
  if (previousIdentifier && previousIdentifier !== identifier) {
    const backup = project + '-previous-' + Date.now();
    renameSync(project, backup);
    console.info('Application identifier changed; previous native project saved at ' + backup);
    const signing = join(backup, 'keystore.properties');
    if (platform === 'android' && existsSync(signing)) previousSigning = signing;
  }
  const marker = platform === 'android'
    ? join(native, 'gen', 'android', 'app', 'src', 'main', 'AndroidManifest.xml')
    : join(native, 'gen', 'apple', 'project.yml');
  if (command === 'init' || !existsSync(marker)) {
    tauri(platform, 'init', '--ci', ...(command === 'init' ? args : []));
    writeFileSync(identityFile, identifier + '\n');
    if (previousSigning) cpSync(previousSigning, join(project, 'keystore.properties'));
  }
  if (platform === 'android') {
    configureAndroid();
  }
  if (platform === 'ios') configureIosIcons(project);
  if (command !== 'init') tauri(platform, command, ...args);
} catch (error) {
  console.error(error.message);
  process.exitCode = 1;
}
