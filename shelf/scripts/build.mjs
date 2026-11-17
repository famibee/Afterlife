// Shelfのビルド。成果物を shelf/dist-local/ へコピーする（OSごとに作るものが違う）。
// 使い方: node scripts/build.mjs gen|app   （package.jsonの gen:build / app:build から呼ぶ）
//
// mac: universalでビルドする。コピーするのは app（ランチャーのスタブ単体のdmg。動作確認用）だけ。
//      gen（生成ツール）はこのリポジトリが手元にあるMacでしか動かず、配布物にならないので
//      コピーしない（普段は `bun run gen:dev` で足りる。ビルドしたappはtarget/内に残る）。
//      gen と app の dmg は同じ target/universal-apple-darwin/release/bundle/dmg/ に出るため、
//      片方をビルドするともう片方が消える点に注意（appはdist-localへ退避して残す）。
// Win: app（ランチャーのスタブ）だけ。生成ツール（gen）はmac専用（hdiutil等を使う）。
//      target/release/shelf.exe を dist-local/shelf.exe にコピーする。これを
//      gen/prebuilt/x86_64-pc-windows-msvc/shelf.exe に置く（blues-sync経由でmacへ）。
import { spawnSync } from 'node:child_process';
import { copyFileSync, mkdirSync, readdirSync, existsSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { homedir } from 'node:os';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const target = process.argv[2];
if (target !== 'gen' && target !== 'app') {
	console.error('gen か app を指定してください');
	process.exit(1);
}

const isMac = process.platform === 'darwin';
const isWin = process.platform === 'win32';
if (!isMac && !isWin) {
	console.error('mac か Windows で実行してください');
	process.exit(1);
}
if (isWin && target === 'gen') {
	console.error('生成ツール（gen）はmac専用です。Windowsでビルドするのはランチャーのスタブ（app）だけです。');
	process.exit(1);
}

// rustup版を優先する（Homebrew版がPATHで勝つ罠。macのみ。Winはrustupだけのはず）
const env = { ...process.env };
if (isMac) {
	const cargoBin = join(homedir(), '.cargo', 'bin');
	env.PATH = `${cargoBin}:${env.PATH}`;
	env.RUSTC = join(cargoBin, 'rustc');
}

const tauriBin = join(root, 'node_modules', '.bin', isWin ? 'tauri.cmd' : 'tauri');
const args = ['build'];
if (isMac) args.push('--target', 'universal-apple-darwin');
const r = spawnSync(tauriBin, args, { cwd: join(root, target), env, stdio: 'inherit', shell: isWin });
if (r.status !== 0) process.exit(r.status ?? 1);

const out = join(root, 'dist-local');
mkdirSync(out, { recursive: true });
const copied = [];
if (isMac && target === 'app') {
	const dmgDir = join(root, 'target', 'universal-apple-darwin', 'release', 'bundle', 'dmg');
	const prefix = 'Shelf_';
	for (const f of readdirSync(dmgDir)) {
		if (f.startsWith(prefix) && f.endsWith('_universal.dmg')) {
			copyFileSync(join(dmgDir, f), join(out, f));
			copied.push(f);
		}
	}
} else if (isWin) {
	const exe = join(root, 'target', 'release', 'shelf.exe');
	if (existsSync(exe)) {
		copyFileSync(exe, join(out, 'shelf.exe'));
		copied.push('shelf.exe');
	}
}
if (copied.length === 0 && target === 'gen') {
	console.log('生成ツールのビルド完了（dist-localへのコピーなし）。target/ 内の .app / .dmg を参照');
	process.exit(0);
}
if (copied.length === 0) {
	console.error('成果物が見つかりません（ビルド出力の場所を確認してください）');
	process.exit(1);
}
console.log(`出力: ${out}`);
for (const f of copied) console.log(`  ${f}`);
