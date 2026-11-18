'use strict';

const {invoke} = window.__TAURI__.core;

// WebView(macOSのWKWebView)ではwindow.alert/confirmが何も表示されず、confirmは常にfalseになる
// （wryが実装していない）。Tauriのダイアログプラグインで代替する。無ければ標準のalert/confirmに戻す
function notify(message) {
	const dlg = window.__TAURI__ && window.__TAURI__.dialog;
	if (dlg && dlg.message) return dlg.message(String(message), {title: 'Shelf'});
	window.alert(message);
	return Promise.resolve();
}

async function askYesNo(message) {
	const dlg = window.__TAURI__ && window.__TAURI__.dialog;
	if (dlg && dlg.ask) return dlg.ask(String(message), {title: 'Shelf', kind: 'warning'});
	return window.confirm(message);
}
const $ = (id) => document.getElementById(id);

// --- タブ切り替え ---
document.querySelectorAll('.tab').forEach((btn) => {
	btn.addEventListener('click', () => {
		document.querySelectorAll('.tab').forEach((b) => b.classList.remove('active'));
		document.querySelectorAll('.panel').forEach((p) => p.classList.remove('active'));
		btn.classList.add('active');
		$(`panel-${btn.dataset.tab}`).classList.add('active');
	});
});

// --- 状態 ---
let circle = {name: '', siteUrl: '', supportUrl: '', email: ''};
let genSettings = {launcherName: '', manifestUrl: '', manualUrl: '', outputDir: '', volumeName: '', uploadLaunchers: false, launcherIconPath: ''};
let apps = [];	// 各要素は下のappCardNodes.get(id)のDOMノードと対になる
let appCounter = 0;
const appCardNodes = new Map();	// id -> DOMノード
let cancelSeq = 0;	// 公開取り消しの通し番号。lastGenerateResult.cancelSeq以下の取り消しだけが、その生成物のマニフェストに反映済み
let lastGenerateResult = null;	// generate_launchersの戻り値（マニフェスト公開ボタンが使う）

let saveTimer = null;
function scheduleSaveState() {
	clearTimeout(saveTimer);
	saveTimer = setTimeout(() => {
		invoke('save_gen_state', {state: {circle, genSettings, apps}}).catch((e) => console.warn('save_gen_state failed', e));
	}, 300);
}

function newApp() {
	appCounter += 1;
	return {
		id: `app-${appCounter}`,
		projectFolder: null,
		appSlug: '',
		appId: '', winInstallDirName: '', displayName: '', summary: '', eventOnly: false,
		storeLabel: '', storeUrl: '',
		settingSnFileName: 'setting.sn', crypto: false,
		iconPngBase64: '',
		checksumSetting: [], checksumInstaller: [],
		pendingDeletes: [],	// 公開取り消し済みで、マニフェスト再公開後にR2から消すもの [{os, prefix, seq}]
		legacyInstallers: [],	// [{path, kind: 'genuine'|'fallback'}]
		mac: {path: '', version: '', arch: 'universal', sha256: '', size: 0, url: ''},
		win: {path: '', version: '', arch: 'x64', sha256: '', size: 0, url: ''},
	};
}

function basename(path) {
	return path.split(/[\\/]/).pop();
}

// --- 作品カードの描画 ---

function renderApp(app) {
	const tmpl = $('tmplApp');
	const node = tmpl.content.firstElementChild.cloneNode(true);

	const bind = (selector, prop, event = 'input') => {
		const el = node.querySelector(selector);
		el.value = app[prop] ?? '';
		el.addEventListener(event, () => { app[prop] = el.value; scheduleSaveState(); });
	};
	bind('.summary', 'summary');
	bind('.storeLabel', 'storeLabel');
	bind('.storeUrl', 'storeUrl');
	bind('.settingSnFileName', 'settingSnFileName');

	const eventOnlyEl = node.querySelector('.eventOnly');
	eventOnlyEl.checked = !!app.eventOnly;
	eventOnlyEl.addEventListener('change', () => { app.eventOnly = eventOnlyEl.checked; scheduleSaveState(); });

	node.querySelector('.appDisplayName').textContent = app.displayName || app.appId || '（未スキャン）';
	node.querySelector('.cryptoNotice').hidden = !app.crypto;
	node.querySelector('.projectFolder').value = app.projectFolder || '';

	renderLegacyInstallerList(node, app);
	for (const os of ['mac', 'win']) {
		const pane = node.querySelector(`.os-pane[data-os="${os}"]`);
		pane.querySelector('.latestPath').value = app[os].path;
		pane.querySelector('.latestVersion').value = app[os].version;
		pane.querySelector('.latestArch').value = app[os].arch;
		pane.querySelector('.latestUrl').value = app[os].url;
	}

	node.querySelector('.btnRecheck').addEventListener('click', () => refreshPatchPane(app, node));
	refreshPatchPane(app, node);
	updateOsAbsent(app, node);

	node.querySelector('.btn-remove-app').addEventListener('click', () => {
		apps = apps.filter((a) => a.id !== app.id);
		appCardNodes.delete(app.id);
		node.remove();
		scheduleSaveState();
	});

	node.querySelector('.btnPickProject').addEventListener('click', async () => {
		const path = await invoke('select_folder');
		if (!path) return;
		await applyProjectFolder(app, node, path);
	});

	node.querySelector('.btnPickLegacyInstallers').addEventListener('click', async () => {
		const macPaths = await invoke('select_installer_files', {ext: 'dmg'});
		const winPaths = await invoke('select_installer_files', {ext: 'exe'});
		for (const path of [...macPaths, ...winPaths]) {
			await addLegacyInstaller(app, path, node);
		}
		scheduleSaveState();
	});

	for (const os of ['mac', 'win']) {
		const pane = node.querySelector(`.os-pane[data-os="${os}"]`);
		pane.querySelector('.btnPickLatest').addEventListener('click', async () => {
			const path = await invoke('select_installer_files', {ext: os === 'mac' ? 'dmg' : 'exe'});
			if (!path || path.length === 0) return;
			await setLatestInstaller(app, os, path[0], node);
			scheduleSaveState();
		});
		pane.querySelector('.btnUploadLatest').addEventListener('click', async () => {
			await uploadLatest(app, os, node);
		});
		pane.querySelector('.btnClearLatest').addEventListener('click', () => cancelOsPublish(app, os, node));
	}

	return node;
}

async function applyProjectFolder(app, node, path) {
	app.projectFolder = path;
	const scan = await invoke('scan_project_folder', {path});
	// 表示名・appId・Winのインストール先フォルダ名は package.json から決まる値。
	// 表示名はmacの<表示名>.app・Winの<表示名>.exeを探すキーなので、手で変えると検出が壊れる。常にスキャン結果に合わせる
	app.displayName = scan.appName;
	app.appId = scan.appSlug;
	app.winInstallDirName = scan.appSlug;	// electron-builderはフォルダ名にpackage.jsonのnameを使う
	app.appSlug = scan.appSlug;
	app.crypto = scan.crypto;
	app.iconPngBase64 = scan.iconPngBase64 || '';
	node.querySelector('.projectFolder').value = path;
	node.querySelector('.appDisplayName').textContent = app.displayName || app.appId;
	node.querySelector('.cryptoNotice').hidden = !app.crypto;
	if (scan.settingSnFileNameGuess) {
		app.settingSnFileName = scan.settingSnFileNameGuess;
		node.querySelector('.settingSnFileName').value = scan.settingSnFileNameGuess;
		node.querySelector('.settingSnGuessNotice').hidden = false;
	}
	else {
		node.querySelector('.settingSnGuessNotice').hidden = true;
	}
	node.querySelector('.settingSnDetails').open = !!app.crypto && !scan.settingSnFileNameGuess;
	node.querySelector('.scanWarnings').textContent = (scan.warnings || []).join(' / ');
	await refreshPatchPane(app, node);
	scheduleSaveState();
}

async function extractChecksumFor(path, settingSnFileName) {
	const isDmg = path.toLowerCase().endsWith('.dmg');
	try {
		const result = isDmg
			? await invoke('extract_setting_sn_from_dmg', {dmgPath: path, settingSnFileName})
			: await invoke('extract_setting_sn_from_nsis_exe', {exePath: path, settingSnFileName});
		return {kind: 'genuine', checksum: result.checksumHex};
	}
	catch (e) {
		// setting.sn自体が読めない（旧エンジンに体験版チェック機構が無い等）。
		// ②フォールバック（インストーラー本体のサンプリングチェックサム）に切り替える
		const checksum = await invoke('installer_checksum', {path});
		return {kind: 'fallback', checksum, error: String(e)};
	}
}

// --- SKYNovel [update_check] 用パッチ（build/package/update 一式） ---

function randomToken(len) {
	const chars = 'abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789';
	const buf = crypto.getRandomValues(new Uint8Array(len));
	return Array.from(buf, (b) => chars[b % chars.length]).join('');
}

// 公開階層: main.snに既にある公開URL配下ならそれを維持（配布済みアプリが見に行く場所なので変えない）。
// 無ければnull（呼び出し側が新規に作る）
function existingPatchPrefix(info, config) {
	const base = (config.publicBaseUrl || '').replace(/\/+$/, '') + '/';
	if (info.currentUrl && config.publicBaseUrl && info.currentUrl.startsWith(base) && info.currentUrl.endsWith('/')) {
		return info.currentUrl.slice(base.length);
	}
	return null;
}

// 新旧チェック（元→暗号化→バンドル→update一式→R2）。R2が未設定・未確認でもローカル分は返る
async function runPatchCheck(app, info) {
	const config = await invoke('r2_load_config').catch(() => null);
	let remoteChecked = false;
	let remoteIndex = null;
	if (config && config.bucket) {
		const prefix = existingPatchPrefix(info, config);
		if (prefix) {
			try {
				remoteIndex = await invoke('r2_get_text', {config, key: `${prefix}_index.json`});
				remoteChecked = true;
			}
			catch (e) {
				const steps = await invoke('patch_check', {projectFolder: app.projectFolder, remoteChecked: false, remoteIndex: null});
				const r2 = steps.find((s) => s.id === 'r2');
				r2.level = 'warn'; r2.notes = [`R2の確認に失敗: ${e}`];
				return steps;
			}
		}
	}
	return invoke('patch_check', {projectFolder: app.projectFolder, remoteChecked, remoteIndex});
}

// 工程ごとのやること（SKYNovel側の作業。このツールからは実行しない）
const PATCH_STEP_HINTS = {
	crypto: '暗号化し直す（BlueSNovel拡張）',
	mac: 'mac版をバンドルする（npm run pkg:mac）',
	win: 'win版をバンドルする（Windows機で npm run pkg:win）',
	index: '配布形式を生成して build/package/update を作り直す',
};
const PATCH_ICON = {error: '✗', warn: '⚠', ok: '✓', skip: '–', todo: '○'};

// 更新フォルダがあるプロジェクトのときだけ欄を出す。
// 手順: 元 → 暗号化 → main.sn の url → mac/win バンドル → update 一式 → R2。
// 上流が下流より新しい工程に印を付け、最初の問題を「次はここ」にする
async function refreshPatchPane(app, node) {
	const pane = node.querySelector('.patch-pane');
	if (!app.projectFolder) { pane.hidden = true; return; }
	const info = await invoke('patch_inspect', {projectFolder: app.projectFolder}).catch(() => null);
	pane.hidden = !info || !info.available;
	if (pane.hidden) return;
	const mb = (info.files.reduce((n, f) => n + f.size, 0) / 1024 / 1024).toFixed(1);
	node.querySelector('.patchInfo').textContent = `v${info.version}・${info.files.length}ファイル（${mb} MB）`;

	const list = node.querySelector('.patchSteps');
	list.textContent = '確認中…';
	let steps;
	try {
		steps = await runPatchCheck(app, info);
	}
	catch (e) {
		list.textContent = `チェックに失敗: ${e}`;
		return;
	}
	const config = await invoke('r2_load_config').catch(() => null);

	// main.sn の url 工程はJS側で判定する（公開URLはR2設定が要るため）
	const mainsn = steps.find((s) => s.id === 'mainsn');
	const hasR2 = !!(config && config.bucket && config.publicBaseUrl);
	const existing = hasR2 ? existingPatchPrefix(info, config) : null;
	let urlAction = null;
	if (!info.mainSnExists) { mainsn.level = 'error'; mainsn.notes.push('doc/prj/script/main.sn が無い'); }
	else if (!hasR2) { mainsn.level = 'skip'; mainsn.notes.push(`現在: ${info.currentUrl || '（未設定）'}（R2設定が無く判定できない）`); }
	else if (existing) { mainsn.notes.push(`現在: ${info.currentUrl}`); }
	else {
		mainsn.level = 'warn';
		mainsn.notes.push(info.currentUrl ? `公開URLの外を指している: ${info.currentUrl}` : '[update_check] の url が未設定');
		mainsn.notes.push('設定したら再バンドルが必要');
		urlAction = async () => {
			app.patchPrefix = app.patchPrefix || `patch/${app.appSlug}/${randomToken(40)}/`;
			const url = await invoke('r2_public_url', {config, key: app.patchPrefix});
			await invoke('patch_set_update_check_url', {projectFolder: app.projectFolder, url});
			scheduleSaveState();
		};
	}

	const next = steps.find((s) => s.level === 'error' || s.level === 'warn');
	const nextEl = node.querySelector('.patchNext');
	nextEl.textContent = next
		? `次にやること: ${next.id === 'r2' ? 'R2へアップロード' : (PATCH_STEP_HINTS[next.id] || (next.id === 'mainsn' ? 'main.sn の url を設定' : next.title))}`
		: '✓ すべて最新です（公開済み）';
	nextEl.className = `patchNext ${next ? 'has-next' : 'all-ok'}`;

	list.textContent = '';
	for (const s of steps) {
		const li = document.createElement('li');
		const isNext = s === next;
		li.className = `step step-${s.level}${isNext ? ' is-next' : ''}`;

		const head = document.createElement('div');
		head.className = 'step-head';
		head.innerHTML = '<span class="step-icon"></span><span class="step-title"></span><span class="step-time"></span>';
		head.querySelector('.step-icon').textContent = PATCH_ICON[s.level];
		head.querySelector('.step-title').textContent = s.title;
		head.querySelector('.step-time').textContent = s.time;
		li.appendChild(head);

		const body = document.createElement('div');
		body.className = 'step-body';
		for (const n of s.notes) {
			const p = document.createElement('div');
			p.className = 'step-note';
			p.textContent = n;
			body.appendChild(p);
		}
		if ((s.level === 'warn' || s.level === 'error') && PATCH_STEP_HINTS[s.id]) {
			const p = document.createElement('div');
			p.className = 'step-hint';
			p.textContent = `→ ${PATCH_STEP_HINTS[s.id]}`;
			body.appendChild(p);
		}
		if (s.id === 'mainsn' && urlAction) {
			const b = document.createElement('button');
			b.type = 'button';
			b.textContent = 'url を設定';
			b.addEventListener('click', async () => {
				b.disabled = true;
				try { await urlAction(); await refreshPatchPane(app, node); }
				catch (e) { node.querySelector('.patchStatus').textContent = `失敗: ${e}`; b.disabled = false; }
			});
			body.appendChild(b);
		}
		if (s.id === 'r2') {
			const b = document.createElement('button');
			b.type = 'button';
			b.textContent = 'R2へアップロード';
			b.className = isNext ? 'primary' : '';
			b.addEventListener('click', () => publishPatch(app, node));
			body.appendChild(b);
		}
		if (body.childNodes.length) li.appendChild(body);
		list.appendChild(li);
	}
	return steps;
}

async function publishPatch(app, node) {
	const status = node.querySelector('.patchStatus');
	const btn = node.querySelector('.btnRecheck');
	if (!app.appSlug) { notify('先にプロジェクトフォルダを選んでスキャンしてください'); return; }
	btn.disabled = true;
	try {
		const config = await invoke('r2_load_config');
		if (!config || !config.bucket) { notify('先に「ホスティング」タブでR2の設定を保存してください'); return; }
		const info = await invoke('patch_inspect', {projectFolder: app.projectFolder});
		if (!info.available) { notify('build/package/update/_index.json が見つかりません'); return; }

		// 公開フォルダ: main.snに既にある公開URL配下ならそれを維持（配布済みアプリが見に行く場所なので変えない）。
		// 無ければ推測されにくいランダム階層を新規に作る
		const prefix = existingPatchPrefix(info, config) || `patch/${app.appSlug}/${randomToken(40)}/`;

		// main.snのurlが変わる場合は、先にmain.snだけ更新して止める。今ある build/package/update は
		// 古いurlの入ったmain.snをバンドルしたものなので、そのまま上げても配布先のアプリは新しいurlを知らない。
		// 再バンドル（dmg・パッチ一式の再生成）のあと、もう一度このボタンを押す
		const url = await invoke('r2_public_url', {config, key: prefix});
		if (info.currentUrl !== url) {
			await invoke('patch_set_update_check_url', {projectFolder: app.projectFolder, url});
			status.textContent = '⚠ main.sn の [update_check] url を更新しました。アップロードはしていません。'
				+ '再バンドルして dmg・パッチ一式を作り直してから、もう一度押してください';
			await refreshPatchPane(app, node);
			return;
		}

		// 新旧チェックの警告があれば、公開してよいか確認する。R2の工程の差は、これから上げて解消するものなので除く
		const steps = (await refreshPatchPane(app, node)) || [];
		const problems = steps.filter((s) => s.id !== 'r2' && s.id !== 'mainsn' && (s.level === 'error' || s.level === 'warn'));
		if (problems.length > 0) {
			const text = problems.map((s) => `・${s.title}: ${s.notes.join(' / ')}`).join('\n');
			if (!(await askYesNo(`新旧チェックで警告があります。それでもR2へ公開しますか？\n\n${text}`))) {
				status.textContent = '中止しました（警告を確認してください）';
				return;
			}
		}

		// _index.jsonは最後（patch_inspectが並べ済み）。途中で失敗しても旧索引が旧ファイルを指したまま残る
		const keys = [];
		let i = 0;
		for (const f of info.files) {
			i += 1;
			status.textContent = `アップロード中 ${i}/${info.files.length}: ${f.name}`;
			const key = prefix + f.name;
			const exists = f.name !== '_index.json' && await invoke('r2_object_exists', {config, key});
			if (!exists) await invoke('r2_upload_file', {config, localPath: f.path, key});
			keys.push(key);
		}
		const deleted = await invoke('r2_delete_not_in', {config, prefix, keepKeys: keys}).catch((e) => { notify(`旧ファイルの削除に失敗: ${e}`); return []; });
		status.textContent = `✓ 公開しました（v${info.version}・旧${deleted.length}件削除${deleted.length ? `: ${deleted.map((k) => k.slice(prefix.length)).join(', ')}` : ''}）`;
		await refreshPatchPane(app, node);
	}
	catch (e) {
		status.textContent = `失敗: ${e}`;
	}
	finally {
		btn.disabled = false;
	}
}

// 公開取り消し: この版を配布しない状態に戻す。
// R2のインストーラーは、マニフェストを再公開（外したOSの入ったランチャー再生成→アップロード）した「後」に削除する。
// 先に消すと、古いマニフェストを読む配布済みランチャーの更新が、消えたファイルを取りに行って失敗するため
async function cancelOsPublish(app, os, node) {
	const label = os === 'mac' ? 'mac' : 'win';
	const published = !!app[os].url;
	const msg = published
		? `${app.displayName || app.appId} の${label}版の公開を取り消しますか？\n\n・マニフェストから${label}版を外します（ランチャーを生成し直し、マニフェストをアップロードしたとき反映）\n・R2のインストーラーは、そのマニフェスト公開のあとで削除します`
		: `${label}版の選択を取り消しますか？`;
	if (!(await askYesNo(msg))) return;
	if (published && app.appSlug) {
		cancelSeq += 1;
		app.pendingDeletes = (app.pendingDeletes || []).filter((d) => d.os !== os);
		app.pendingDeletes.push({os, prefix: `games/${app.appSlug}/${os}/`, seq: cancelSeq});
	}
	const pane = node.querySelector(`.os-pane[data-os="${os}"]`);
	app[os] = {path: '', version: '', arch: os === 'mac' ? 'universal' : 'x64', sha256: '', size: 0, url: ''};
	pane.querySelector('.latestPath').value = '';
	pane.querySelector('.latestVersion').value = '';
	pane.querySelector('.latestArch').value = app[os].arch;
	pane.querySelector('.latestUrl').value = '';
	pane.querySelector('.uploadStatus').textContent = '';
	pane.querySelector('.extractNotice').hidden = true;
	updateOsAbsent(app, node);
	scheduleSaveState();
}

// そのOSの最新版が未登録（＝配布しない）ときの説明を出し、クリア・アップロードのボタンを出し分ける
function updateOsAbsent(app, node) {
	for (const os of ['mac', 'win']) {
		const pane = node.querySelector(`.os-pane[data-os="${os}"]`);
		const absent = !app[os].path && !app[os].url;
		const pending = (app.pendingDeletes || []).some((d) => d.os === os);
		const hint = pane.querySelector('.osAbsent');
		hint.hidden = !absent;
		hint.textContent = pending
			? '公開取り消し済み：ランチャーを生成し直してマニフェストをアップロードすると、R2のインストーラーも削除されます'
			: '未登録：このOS向けは配布しません（ランチャーでは「このOS向けの更新なし」と表示）';
		pane.querySelector('.btnClearLatest').hidden = absent;
		pane.classList.toggle('os-absent', absent);
	}
}

async function addLegacyInstaller(app, path, node) {
	const result = await extractChecksumFor(path, app.settingSnFileName);
	if (result.kind === 'genuine') {
		if (!app.checksumSetting.includes(result.checksum)) app.checksumSetting.push(result.checksum);
	}
	else {
		if (!app.checksumInstaller.includes(result.checksum)) app.checksumInstaller.push(result.checksum);
	}
	app.legacyInstallers.push({path, kind: result.kind, error: result.error || ''});
	renderLegacyInstallerList(node, app);
}

function renderLegacyInstallerList(node, app) {
	const list = node.querySelector('.legacyInstallerList');
	list.textContent = '';
	for (const entry of app.legacyInstallers) {
		const li = document.createElement('li');
		const label = entry.kind === 'genuine' ? '✓ setting.sn確認済み' : '⚠ フォールバック（本体チェックサムのみ）';
		li.textContent = `${basename(entry.path)} — ${label}`;
		if (entry.error) li.title = entry.error;
		list.appendChild(li);
	}
}

async function setLatestInstaller(app, os, path, node) {
	const pane = node.querySelector(`.os-pane[data-os="${os}"]`);
	app[os].path = path;
	app[os].url = '';	// 新しいファイルを選んだら古いURL表示はクリアする
	pane.querySelector('.latestPath').value = path;
	pane.querySelector('.latestUrl').value = '';

	// ファイル名（例: sn_kowloon-1.2.0-x64.dmg）から版とarchを推定して入れる。
	// 版・archは実ファイルと食い違うと更新判定を誤るので、読み取れたら常に合わせる（編集欄は無い）
	const fname = basename(path);
	const ver = fname.match(/(\d+\.\d+(?:\.\d+)?)/);
	if (ver) {
		app[os].version = ver[1];
		pane.querySelector('.latestVersion').value = ver[1];
	}
	const archMatch = fname.match(/(universal|arm64|aarch64|x86_64|x64)/i);
	if (archMatch) {
		const arch = {aarch64: 'arm64', x86_64: 'x64'}[archMatch[1].toLowerCase()] || archMatch[1].toLowerCase();
		app[os].arch = arch;
		pane.querySelector('.latestArch').value = arch;
	}

	const checksumResult = await extractChecksumFor(path, app.settingSnFileName);
	if (checksumResult.kind === 'genuine' && !app.checksumSetting.includes(checksumResult.checksum)) {
		app.checksumSetting.push(checksumResult.checksum);
	}
	else if (checksumResult.kind === 'fallback' && !app.checksumInstaller.includes(checksumResult.checksum)) {
		app.checksumInstaller.push(checksumResult.checksum);
	}
	// setting.snを読めずフォールバックした理由（7-Zip未導入など）を見える所に出す
	const notice = pane.querySelector('.extractNotice');
	notice.hidden = checksumResult.kind !== 'fallback';
	notice.textContent = checksumResult.kind === 'fallback'
		? `⚠ setting.snを抽出できなかったため、購入者チェックは本体チェックサムで代替します（${checksumResult.error}）`
		: '';

	const info = await invoke('file_info', {path});
	app[os].sha256 = info.sha256;
	app[os].size = info.size;
	updateOsAbsent(app, node);
}

async function uploadLatest(app, os, node) {
	const pane = node.querySelector(`.os-pane[data-os="${os}"]`);
	const status = pane.querySelector('.uploadStatus');
	if (!app.appSlug) { notify('先にプロジェクトフォルダを選んでスキャンしてください'); return; }
	if (!app[os].path) { notify('先にインストーラーを選んでください'); return; }

	status.textContent = 'アップロード中…';
	try {
		const config = await invoke('r2_load_config');
		if (!config || !config.bucket) { notify('先に「ホスティング」タブでR2の設定を保存してください'); status.textContent = ''; return; }

		const filename = basename(app[os].path);
		const prefix = `games/${app.appSlug}/${os}/`;
		const key = `${prefix}${app[os].sha256.slice(0, 16)}/${filename}`;

		const exists = await invoke('r2_object_exists', {config, key});
		let url;
		if (exists) {
			url = await invoke('r2_public_url', {config, key});
			status.textContent = '✓ スキップ（内容不変）';
		}
		else {
			url = await invoke('r2_upload_file', {config, localPath: app[os].path, key});
			status.textContent = '✓ アップロード完了';
		}
		app[os].url = url;
		app.pendingDeletes = (app.pendingDeletes || []).filter((d) => d.os !== os);
		updateOsAbsent(app, node);
		pane.querySelector('.latestUrl').value = url;

		try {
			const deleted = await invoke('r2_delete_others_with_prefix', {config, prefix, keepKey: key});
			if (deleted > 0) status.textContent += `（旧版${deleted}件を削除）`;
		}
		catch (e) {
			status.textContent += `（旧版の削除に失敗: ${e}）`;
		}
		scheduleSaveState();
	}
	catch (e) {
		status.textContent = `失敗: ${e}`;
	}
}

function addAppCard(app) {
	apps.push(app);
	const node = renderApp(app);
	appCardNodes.set(app.id, node);
	$('apps').appendChild(node);
}

$('btnAddApp').addEventListener('click', () => { addAppCard(newApp()); scheduleSaveState(); });

// --- 生成 ---

$('btnPickOutputDir').addEventListener('click', async () => {
	const path = await invoke('select_folder');
	if (!path) return;
	genSettings.outputDir = path;
	$('outputDir').value = path;
	scheduleSaveState();
});

$('btnPickLauncherIcon').addEventListener('click', async () => {
	const path = await invoke('select_image_file');
	if (!path) return;
	genSettings.launcherIconPath = path;
	$('launcherIconPath').value = path;
	scheduleSaveState();
});
$('btnClearLauncherIcon').addEventListener('click', () => {
	genSettings.launcherIconPath = '';
	$('launcherIconPath').value = '';
	scheduleSaveState();
});

for (const [id, key] of [['launcherName', 'launcherName'], ['manifestUrl', 'manifestUrl'], ['manualUrl', 'manualUrl'], ['volumeName', 'volumeName']]) {
	$(id).addEventListener('input', () => { genSettings[key] = $(id).value; scheduleSaveState(); });
}
$('uploadLaunchers').addEventListener('change', () => { genSettings.uploadLaunchers = $('uploadLaunchers').checked; scheduleSaveState(); });

async function uploadGeneratedLaunchers() {
	const status = $('uploadLaunchersStatus');
	status.textContent = 'アップロード中…';
	try {
		const config = await invoke('r2_load_config');
		if (!config || !config.bucket) { notify('先に「ホスティング」タブでR2の設定を保存してください'); status.textContent = ''; return; }

		const messages = [];
		for (const [os, path] of [['mac', lastGenerateResult.macDmgPath], ['win', lastGenerateResult.winExePath]]) {
			if (!path) continue;
			const info = await invoke('file_info', {path});
			const filename = basename(path);
			// gamesの更新用インストーラーとは別名前空間（launcher/）に置く。
			// 一覧から両者が同じ場所にまとまって見えないようにするため
			const prefix = `launcher/${os}/`;
			const key = `${prefix}${info.sha256.slice(0, 16)}/${filename}`;

			const exists = await invoke('r2_object_exists', {config, key});
			const url = exists ? await invoke('r2_public_url', {config, key}) : await invoke('r2_upload_file', {config, localPath: path, key});
			const deleted = await invoke('r2_delete_others_with_prefix', {config, prefix, keepKey: key}).catch(() => 0);
			messages.push(`${os}: ${url}${deleted ? `（旧版${deleted}件を削除）` : ''}`);
		}
		status.textContent = `✓ ${messages.join(' / ')}`;
	}
	catch (e) {
		status.textContent = `失敗: ${e}`;
	}
}

$('btnUploadLaunchers').addEventListener('click', uploadGeneratedLaunchers);

$('btnGenerate').addEventListener('click', async () => {
	const log = $('generateLog');
	log.textContent = '生成中…（mac universalビルドは数分かかります）';
	$('btnGenerate').disabled = true;
	try {
		const request = {
			launcherName: genSettings.launcherName,
			manifestUrl: genSettings.manifestUrl,
			manualUrl: genSettings.manualUrl,
			circle,
			outputDir: genSettings.outputDir,
			launcherIconPath: genSettings.launcherIconPath || null,
			volumeName: genSettings.volumeName || genSettings.launcherName || 'Shelf',
			apps: apps.map((a) => ({
				appId: a.appId,
				appName: a.displayName,
				winInstallDirName: a.winInstallDirName || null,
				settingSnFileName: a.settingSnFileName,
				checksumSetting: a.checksumSetting,
				checksumInstaller: a.checksumInstaller,
				name: a.displayName,
				summary: a.summary,
				iconPngBase64: a.iconPngBase64,
				version: a.mac.version || a.win.version,
				mac: a.mac.url ? {arch: a.mac.arch, kind: 'dmg', sha256: a.mac.sha256, size: a.mac.size, url: a.mac.url} : null,
				win: a.win.url ? {arch: a.win.arch, kind: 'nsis', sha256: a.win.sha256, size: a.win.size, url: a.win.url} : null,
				store: a.storeUrl ? {label: a.storeLabel || '配布サイト', url: a.storeUrl} : null,
				eventOnly: a.eventOnly,
			})),
		};
		const result = await invoke('generate_launchers', {request});
		lastGenerateResult = result;
		lastGenerateResult.cancelSeq = cancelSeq;
		const lines = [
			result.macDmgPath ? `✅ mac: ${result.macDmgPath}` : '✗ mac: 生成失敗',
			result.winExePath ? `✅ win: ${result.winExePath}` : `⚠ win: ${result.winSkippedReason}`,
			'マニフェストの組み立てが完了しました。「ホスティング」タブでアップロードしてください。',
		];
		for (const a of apps) {
			const has = [a.mac.url && 'mac', a.win.url && 'win'].filter(Boolean);
			const name = a.displayName || a.appId;
			if (has.length === 0) lines.push(a.eventOnly ? `ℹ ${name}: mac/winとも版なし（イベント配布のみ）` : `⚠ ${name}: mac/winとも未登録（R2へアップロードした最新版がありません）`);
			else if (has.length === 1) lines.push(`ℹ ${name}: ${has[0]}版のみ（もう一方のOSでは更新を案内しません）`);
			if (a.mac.version && a.win.version && a.mac.version !== a.win.version) lines.push(`⚠ ${name}: macの版（${a.mac.version}）とwinの版（${a.win.version}）が違います。ランチャーにはmacの版が使われます`);
		}
		log.textContent = lines.join('\n');

		$('uploadLaunchersStatus').textContent = '';
		if (genSettings.uploadLaunchers) {
			$('btnUploadLaunchers').hidden = false;
			await uploadGeneratedLaunchers();
		}
		else {
			$('btnUploadLaunchers').hidden = true;
		}
	}
	catch (e) {
		log.textContent = `✗ 失敗: ${e}`;
	}
	finally {
		$('btnGenerate').disabled = false;
	}
});

// --- ホスティング（R2） ---

function r2CollectConfig() {
	return {
		accountId: $('r2AccountId').value.trim(),
		bucket: $('r2Bucket').value.trim(),
		accessKeyId: $('r2AccessKeyId').value.trim(),
		secretAccessKey: $('r2SecretAccessKey').value.trim(),
		publicBaseUrl: $('r2PublicBaseUrl').value.trim(),
	};
}

function r2ApplyConfig(config) {
	if (!config) return;
	$('r2AccountId').value = config.accountId || '';
	$('r2Bucket').value = config.bucket || '';
	$('r2AccessKeyId').value = config.accessKeyId || '';
	$('r2SecretAccessKey').value = config.secretAccessKey || '';
	$('r2PublicBaseUrl').value = config.publicBaseUrl || '';
}

// マニフェストURLは公開ベースURL+/manifest.json が通例。空のときだけ自動で入れる（別の場所に置くなら手で直せる）
function autofillManifestUrl(config) {
	if (!config || !config.publicBaseUrl || genSettings.manifestUrl) return;
	genSettings.manifestUrl = `${config.publicBaseUrl.replace(/\/+$/, '')}/manifest.json`;
	$('manifestUrl').value = genSettings.manifestUrl;
	scheduleSaveState();
}

$('btnR2Save').addEventListener('click', async () => {
	try {
		await invoke('r2_save_config', {config: r2CollectConfig()});
		autofillManifestUrl(r2CollectConfig());
		$('r2SaveStatus').textContent = '✓ 保存しました';
	}
	catch (e) {
		$('r2SaveStatus').textContent = `失敗: ${e}`;
	}
});

$('btnPublishManifest').addEventListener('click', async () => {
	const status = $('publishStatus');
	if (!lastGenerateResult) { notify('先に「作品管理」タブでランチャーを生成してください'); return; }
	status.textContent = 'アップロード中…';
	try {
		const config = r2CollectConfig();
		const manifestBytes = Array.from(new TextEncoder().encode(lastGenerateResult.manifestJson));
		await invoke('r2_upload_bytes', {config, bytes: manifestBytes, key: 'manifest.json'});
		// 署名はbase64のテキストのまま置く（ランチャーはテキストとして読んでbase64デコードする。
		// デコード済みの生バイトを置くと署名検証に失敗し、カードが「確認できません」になる）
		const sigBytes = Array.from(new TextEncoder().encode(lastGenerateResult.manifestSignatureB64));
		await invoke('r2_upload_bytes', {config, bytes: sigBytes, key: 'manifest.json.sig'});
		// マニフェストから外したOSのインストーラーを、公開が済んだ今になって消す
		// （このマニフェストの生成後に取り消したものは、まだ参照されているので消さない）
		const removed = [];
		for (const a of apps) {
			const keep = [];
			for (const d of a.pendingDeletes || []) {
				if (a[d.os] && a[d.os].url) continue;	// 取り消し後に同じOSを上げ直した。新しいファイルを消さない
				if (d.seq > (lastGenerateResult.cancelSeq || 0)) { keep.push(d); continue; }
				try {
					const keys = await invoke('r2_delete_not_in', {config, prefix: d.prefix, keepKeys: []});
					removed.push(...keys);
				}
				catch (e) { keep.push(d); notify(`R2の削除に失敗（${d.prefix}）: ${e}`); }
			}
			a.pendingDeletes = keep;
			const node = appCardNodes.get(a.id);
			if (node) updateOsAbsent(a, node);
		}
		scheduleSaveState();
		status.textContent = `✓ 公開しました（manifest.json・manifest.json.sig）${removed.length ? `／取り消した版のインストーラー${removed.length}件をR2から削除` : ''}`;
	}
	catch (e) {
		status.textContent = `失敗: ${e}`;
	}
});

$('btnR2Refresh').addEventListener('click', async () => {
	const tbody = $('r2List').querySelector('tbody');
	tbody.textContent = '';
	try {
		const config = r2CollectConfig();
		const objects = await invoke('r2_list_objects', {config, prefix: null});
		for (const obj of objects) {
			const tr = document.createElement('tr');
			const sizeMb = (obj.size / 1024 / 1024).toFixed(2);
			tr.innerHTML = `<td>${obj.key}</td><td>${sizeMb} MB</td><td>${obj.lastModified}</td><td></td>`;
			const delBtn = document.createElement('button');
			delBtn.type = 'button';
			delBtn.textContent = '削除';
			delBtn.addEventListener('click', async () => {
				if (!(await askYesNo(`${obj.key} を削除しますか？`))) return;
				await invoke('r2_delete_object', {config, key: obj.key});
				$('btnR2Refresh').click();
			});
			tr.lastElementChild.appendChild(delBtn);
			tbody.appendChild(tr);
		}
	}
	catch (e) {
		notify(`一覧取得に失敗: ${e}`);
	}
});

// --- 起動時の読み込み ---

async function loadInitialState() {
	const state = await invoke('load_gen_state').catch(() => null);
	if (state) {
		circle = state.circle || circle;
		genSettings = state.genSettings || genSettings;
		$('circleName').value = circle.name || '';
		$('circleSiteUrl').value = circle.siteUrl || '';
		$('circleSupportUrl').value = circle.supportUrl || '';
		$('circleEmail').value = circle.email || '';
		$('launcherName').value = genSettings.launcherName || '';
		$('manifestUrl').value = genSettings.manifestUrl || '';
		$('manualUrl').value = genSettings.manualUrl || '';
		$('outputDir').value = genSettings.outputDir || '';
		$('volumeName').value = genSettings.volumeName || '';
		$('uploadLaunchers').checked = !!genSettings.uploadLaunchers;
		$('launcherIconPath').value = genSettings.launcherIconPath || '';

		for (const saved of state.apps || []) {
			for (const d of saved.pendingDeletes || []) cancelSeq = Math.max(cancelSeq, d.seq || 0);
			addAppCard(saved);
			appCounter = Math.max(appCounter, Number(String(saved.id).split('-')[1]) || 0);
		}
	}
	if (apps.length === 0) addAppCard(newApp());

	for (const [id, key] of [['circleName', 'name'], ['circleSiteUrl', 'siteUrl'], ['circleSupportUrl', 'supportUrl'], ['circleEmail', 'email']]) {
		$(id).addEventListener('input', () => { circle[key] = $(id).value; scheduleSaveState(); });
	}

	const r2config = await invoke('r2_load_config').catch(() => null);
	r2ApplyConfig(r2config);
	autofillManifestUrl(r2config);

	try {
		$('signingPublicKey').value = await invoke('load_signing_public_key');
	}
	catch (e) {
		$('signingPublicKey').value = `（取得失敗: ${e}）`;
	}
}

loadInitialState();

// --- ドラッグ＆ドロップ ---
// Tauri（WebView）はドロップされたファイルのパスをDOMのdropイベントでは渡さないため、
// ウィンドウ側のonDragDropEventを使う。ドロップ位置のカードを探して振り分ける:
// フォルダ（.dmg/.exe以外）=プロジェクトフォルダ／.dmg/.exe: 最新版ペイン上=その版、それ以外=旧版インストーラー
function cardAt(position) {
	const ratio = window.devicePixelRatio || 1;
	const el = document.elementFromPoint(position.x / ratio, position.y / ratio);
	const card = el && el.closest('.app-card');
	if (!card) return null;
	for (const [id, node] of appCardNodes) {
		if (node === card || node.contains(card)) return {app: apps.find((a) => a.id === id), node, el};
	}
	return null;
}

function setDragHighlight(position) {
	document.querySelectorAll('.app-card.dragover').forEach((c) => c.classList.remove('dragover'));
	if (!position) return;
	const hit = cardAt(position);
	if (hit) hit.el.closest('.app-card').classList.add('dragover');
}

window.__TAURI__.webviewWindow.getCurrentWebviewWindow().onDragDropEvent(async (event) => {
	const p = event.payload;
	if (p.type === 'over') { setDragHighlight(p.position); return; }
	setDragHighlight(null);
	if (p.type !== 'drop') return;
	const hit = cardAt(p.position);
	if (!hit || !hit.app) return;
	const {app, node, el} = hit;
	try {
		const isInstaller = (x) => /\.(dmg|exe)$/i.test(x);
		const folders = p.paths.filter((x) => !isInstaller(x));
		const installers = p.paths.filter(isInstaller);
		if (folders.length > 0) await applyProjectFolder(app, node, folders[0]);
		const pane = el.closest('.os-pane');
		for (const path of installers) {
			const os = path.toLowerCase().endsWith('.dmg') ? 'mac' : 'win';
			if (pane && pane.dataset.os === os) await setLatestInstaller(app, os, path, node);
			else if (!pane) await addLegacyInstaller(app, path, node);
		}
		scheduleSaveState();
	}
	catch (e) {
		notify(`ドロップの処理に失敗しました：${e}`);
	}
});
