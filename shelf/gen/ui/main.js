'use strict';

const {invoke} = window.__TAURI__.core;
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
let genSettings = {launcherName: '', manifestUrl: '', manualUrl: '', outputDir: '', volumeName: '', uploadLaunchers: false};
let apps = [];	// 各要素は下のappCardNodes.get(id)のDOMノードと対になる
let appCounter = 0;
const appCardNodes = new Map();	// id -> DOMノード
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
		appId: '', displayName: '', summary: '', eventOnly: false,
		storeLabel: '', storeUrl: '',
		settingSnFileName: 'setting.sn', crypto: false,
		iconPngBase64: '',
		checksumSetting: [], checksumInstaller: [],
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
	bind('.appId', 'appId');
	bind('.displayName', 'displayName');
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
		pane.querySelector('.latestVersion').addEventListener('input', (e) => { app[os].version = e.target.value; scheduleSaveState(); });
		pane.querySelector('.latestArch').value = app[os].arch;
		pane.querySelector('.latestArch').addEventListener('input', (e) => { app[os].arch = e.target.value; scheduleSaveState(); });
		pane.querySelector('.latestUrl').value = app[os].url;
	}

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
	}

	return node;
}

async function applyProjectFolder(app, node, path) {
	app.projectFolder = path;
	const scan = await invoke('scan_project_folder', {path});
	app.displayName = app.displayName || scan.appName;
	app.appId = app.appId || scan.appSlug;
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
	node.querySelector('.scanWarnings').textContent = (scan.warnings || []).join(' / ');
	if (!app.appId) node.querySelector('.appId').value = app.appId;
	if (!node.querySelector('.displayName').value) node.querySelector('.displayName').value = app.displayName;
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

async function addLegacyInstaller(app, path, node) {
	const result = await extractChecksumFor(path, app.settingSnFileName);
	if (result.kind === 'genuine') {
		if (!app.checksumSetting.includes(result.checksum)) app.checksumSetting.push(result.checksum);
	}
	else {
		if (!app.checksumInstaller.includes(result.checksum)) app.checksumInstaller.push(result.checksum);
	}
	app.legacyInstallers.push({path, kind: result.kind});
	renderLegacyInstallerList(node, app);
}

function renderLegacyInstallerList(node, app) {
	const list = node.querySelector('.legacyInstallerList');
	list.textContent = '';
	for (const entry of app.legacyInstallers) {
		const li = document.createElement('li');
		const label = entry.kind === 'genuine' ? '✓ setting.sn確認済み' : '⚠ フォールバック（本体チェックサムのみ）';
		li.textContent = `${basename(entry.path)} — ${label}`;
		list.appendChild(li);
	}
}

async function setLatestInstaller(app, os, path, node) {
	const pane = node.querySelector(`.os-pane[data-os="${os}"]`);
	app[os].path = path;
	app[os].url = '';	// 新しいファイルを選んだら古いURL表示はクリアする
	pane.querySelector('.latestPath').value = path;
	pane.querySelector('.latestUrl').value = '';

	const checksumResult = await extractChecksumFor(path, app.settingSnFileName);
	if (checksumResult.kind === 'genuine' && !app.checksumSetting.includes(checksumResult.checksum)) {
		app.checksumSetting.push(checksumResult.checksum);
	}
	else if (checksumResult.kind === 'fallback' && !app.checksumInstaller.includes(checksumResult.checksum)) {
		app.checksumInstaller.push(checksumResult.checksum);
	}

	const info = await invoke('file_info', {path});
	app[os].sha256 = info.sha256;
	app[os].size = info.size;
}

async function uploadLatest(app, os, node) {
	const pane = node.querySelector(`.os-pane[data-os="${os}"]`);
	const status = pane.querySelector('.uploadStatus');
	if (!app.appSlug) { alert('先にプロジェクトフォルダを選んでスキャンしてください'); return; }
	if (!app[os].path) { alert('先にインストーラーを選んでください'); return; }

	status.textContent = 'アップロード中…';
	try {
		const config = await invoke('r2_load_config');
		if (!config || !config.bucket) { alert('先に「ホスティング」タブでR2の設定を保存してください'); status.textContent = ''; return; }

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

for (const [id, key] of [['launcherName', 'launcherName'], ['manifestUrl', 'manifestUrl'], ['manualUrl', 'manualUrl'], ['volumeName', 'volumeName']]) {
	$(id).addEventListener('input', () => { genSettings[key] = $(id).value; scheduleSaveState(); });
}
$('uploadLaunchers').addEventListener('change', () => { genSettings.uploadLaunchers = $('uploadLaunchers').checked; scheduleSaveState(); });

async function uploadGeneratedLaunchers() {
	const status = $('uploadLaunchersStatus');
	status.textContent = 'アップロード中…';
	try {
		const config = await invoke('r2_load_config');
		if (!config || !config.bucket) { alert('先に「ホスティング」タブでR2の設定を保存してください'); status.textContent = ''; return; }

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
			volumeName: genSettings.volumeName || genSettings.launcherName || 'Shelf',
			apps: apps.map((a) => ({
				appId: a.appId,
				appName: a.displayName,
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
		const lines = [
			result.macDmgPath ? `✅ mac: ${result.macDmgPath}` : '✗ mac: 生成失敗',
			result.winExePath ? `✅ win: ${result.winExePath}` : `⚠ win: ${result.winSkippedReason}`,
			'マニフェストの組み立てが完了しました。「ホスティング」タブでアップロードしてください。',
		];
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

$('btnR2Save').addEventListener('click', async () => {
	try {
		await invoke('r2_save_config', {config: r2CollectConfig()});
		$('r2SaveStatus').textContent = '✓ 保存しました';
	}
	catch (e) {
		$('r2SaveStatus').textContent = `失敗: ${e}`;
	}
});

function base64ToBytes(b64) {
	const binary = atob(b64);
	return Array.from(binary, (c) => c.charCodeAt(0));
}

$('btnPublishManifest').addEventListener('click', async () => {
	const status = $('publishStatus');
	if (!lastGenerateResult) { alert('先に「作品管理」タブでランチャーを生成してください'); return; }
	status.textContent = 'アップロード中…';
	try {
		const config = r2CollectConfig();
		const manifestBytes = Array.from(new TextEncoder().encode(lastGenerateResult.manifestJson));
		await invoke('r2_upload_bytes', {config, bytes: manifestBytes, key: 'manifest.json'});
		const sigBytes = base64ToBytes(lastGenerateResult.manifestSignatureB64);
		await invoke('r2_upload_bytes', {config, bytes: sigBytes, key: 'manifest.json.sig'});
		status.textContent = '✓ 公開しました（manifest.json・manifest.json.sig）';
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
				if (!confirm(`${obj.key} を削除しますか？`)) return;
				await invoke('r2_delete_object', {config, key: obj.key});
				$('btnR2Refresh').click();
			});
			tr.lastElementChild.appendChild(delBtn);
			tbody.appendChild(tr);
		}
	}
	catch (e) {
		alert(`一覧取得に失敗: ${e}`);
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

		for (const saved of state.apps || []) {
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
		alert(`ドロップの処理に失敗しました：${e}`);
	}
});
