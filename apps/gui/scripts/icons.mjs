import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const gui = fileURLToPath(new URL('../', import.meta.url));
const native = join(gui, 'src-tauri', 'icons');
const markPath = join(gui, 'public', 'omnistream-mark.svg');
const scriptPath = fileURLToPath(import.meta.url);
const fingerprint = () => createHash('sha256').update(readFileSync(markPath)).update(readFileSync(scriptPath)).digest('hex');

function tauriIcon(input, output, sizes = []) {
  const result = spawnSync(process.platform === 'win32' ? 'pnpm.cmd' : 'pnpm',
    ['exec', 'tauri', 'icon', input, '--output', output, ...sizes.flatMap(size => ['--png', String(size)])],
    { cwd: gui, stdio: 'inherit', shell: process.platform === 'win32' });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error('Icon generation failed.');
}

export function generateIcons() {
  const temporary = mkdtempSync(join(tmpdir(), 'omnistream-icons-'));
  try {
    const mark = readFileSync(markPath, 'utf8');
    const symbol = mark.match(/<g id="omnistream-symbol">([\s\S]*?)<\/g>/)?.[1];
    if (!symbol) throw new Error('Missing omnistream-symbol in omnistream-mark.svg.');
    const background = mark.match(/<rect\b[^>]*fill="([^"]+)"/)?.[1];
    if (!background) throw new Error('Missing icon background color.');
    const svg = (size, content) => '<svg xmlns="http://www.w3.org/2000/svg" width="' + size + '" height="' + size + '" viewBox="0 0 ' + size + ' ' + size + '">' + content + '</svg>';
    // The symbol's visual bounds are 10.5..41.5 × 14.5..37.5, centered at 26,26.
    const centered = '<g transform="translate(24 24) scale(.9) translate(-26 -26)">' + symbol + '</g>';
    const app = join(temporary, 'app.svg');
    const foreground = join(temporary, 'android-foreground.svg');
    const androidBackground = join(temporary, 'android-background.svg');
    const maskable = join(temporary, 'maskable.svg');
    writeFileSync(app, svg(48, '<rect x="2" y="2" width="44" height="44" rx="11" fill="' + background + '"/>' + centered));
    // Adaptive icons have a 108-unit canvas. Keep all colored artwork inside
    // the central safe area; the launcher supplies the final background mask.
    writeFileSync(foreground, svg(108, '<g transform="translate(54 54) scale(1.5) translate(-26 -26)">' + symbol + '</g>'));
    writeFileSync(androidBackground, svg(108, '<rect width="108" height="108" fill="' + background + '"/>'));
    // Maskable web icons and Apple touch icons need an opaque, full background.
    writeFileSync(maskable, svg(48, '<rect width="48" height="48" fill="' + background + '"/>' + centered));
    const manifest = join(temporary, 'icons.json');
    writeFileSync(manifest, JSON.stringify({ default: 'app.svg', bg_color: background,
      android_bg: 'android-background.svg', android_fg: 'android-foreground.svg', android_fg_scale: 100 }));
    const output = join(temporary, 'generated');
    tauriIcon(manifest, output);
    mkdirSync(native, { recursive: true });
    for (const platform of ['android', 'ios']) {
      // These are generated, ignored asset folders; remove old variants too.
      rmSync(join(native, platform), { recursive: true, force: true });
      cpSync(join(output, platform), join(native, platform), { recursive: true });
    }
    for (const [name, source] of Object.entries({ 'omnistream-32.png': '32x32.png', 'omnistream-128.png': '128x128.png',
      'omnistream-256.png': '128x128@2x.png', 'omnistream-512.png': 'icon.png', 'icon.ico': 'icon.ico', 'icon.icns': 'icon.icns' })) {
      const path = join(output, source);
      if (name.endsWith('.png')) {
        const png = readFileSync(path);
        if (png[24] !== 8 || png[25] !== 6) throw new Error('Tauri application icons must use RGBA8 PNG.');
      }
      cpSync(path, join(native, name));
    }
    const web = join(temporary, 'web');
    tauriIcon(app, web, [16, 32, 192, 512]);
    const publicIcons = join(gui, 'public', 'icons');
    mkdirSync(publicIcons, { recursive: true });
    for (const size of [16, 32, 192, 512]) cpSync(join(web, size + 'x' + size + '.png'), join(publicIcons, 'omnistream-' + size + '.png'));
    const masked = join(temporary, 'masked');
    tauriIcon(maskable, masked, [180, 512]);
    cpSync(join(masked, '180x180.png'), join(publicIcons, 'omnistream-180.png'));
    cpSync(join(masked, '512x512.png'), join(publicIcons, 'omnistream-maskable-512.png'));
    cpSync(join(native, 'icon.ico'), join(gui, 'public', 'favicon.ico'));
    const hash = fingerprint();
    for (const platform of ['android', 'ios']) writeFileSync(join(native, platform, '.source-hash'), hash);
  } finally { rmSync(temporary, { recursive: true, force: true }); }
}

export function ensureIcons() {
  const hash = fingerprint();
  if (['android', 'ios'].some(platform => {
    const stamp = join(native, platform, '.source-hash');
    return !existsSync(stamp) || readFileSync(stamp, 'utf8') !== hash;
  })) generateIcons();
}

if (process.argv[1] && resolve(process.argv[1]) === scriptPath) generateIcons();
