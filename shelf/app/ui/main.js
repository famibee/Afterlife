'use strict';

// カードUI本体。判定ロジックは全てRust側（shelf_core）にあり、ここは表示と
// ボタン操作の配線だけ（旧 patch_app/ui と同じ作り）。

const { invoke } = window.__TAURI__.core;

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
const { listen } = window.__TAURI__.event;
const $ = (id) => document.getElementById(id);

const STATUS_LABEL = {
	checking: '確認中…',
	upToDate: '✓ 最新です',
	updateAvailable: '更新あり',
	archMigration: '更新あり（移行）',
	notGenuine: '製品版を確認できません',
	needsConfirmation: '確認が必要',
	notInstalled: '',
	notInstalledEventOnly: 'イベント配布のみ',
	manifestError: '確認できません',
	manifestErrorInstalled: '確認できません',
};

const BADGE_CLASS = {
	upToDate: 'badge-ok',
	updateAvailable: 'badge-warn',
	archMigration: 'badge-warn',
	notGenuine: 'badge-error',
	needsConfirmation: 'badge-error',
	manifestError: 'badge-error',
	manifestErrorInstalled: 'badge-error',
};

// 上段（何らかのローカルインストールを検出できている）に出す状態。
// needsConfirmationも「フォルダは見つかったが中身を自動検証できない」だけなので上段に出す
const LOCAL_INSTALL_STATES = new Set([
	'upToDate', 'updateAvailable', 'archMigration', 'notGenuine', 'needsConfirmation', 'manifestErrorInstalled',
]);

// 「起動」ボタンを出す状態。needsConfirmationは意図的に含めない（自動検出そのものに
// 失敗しており、起動対象として確からしいとすら言えないため。「当時のインストーラーを
// 選ぶ」のみを案内する。設計表＝計画の「カードの状態」参照）
const CAN_LAUNCH_STATES = new Set([
	'upToDate', 'updateAvailable', 'archMigration', 'notGenuine', 'manifestErrorInstalled',
]);

const cardNodes = new Map();	// appId -> カードのDOMノード
let manualUrl = '';

function el(tag, text, className) {
	const node = document.createElement(tag);
	if (text != null) node.textContent = text;
	if (className) node.className = className;
	return node;
}

function makeButton(label, handler) {
	const btn = el('button', label, 'btnAction');
	btn.type = 'button';
	btn.addEventListener('click', handler);
	return btn;
}

async function onLaunchClick(card, btn) {
	btn.disabled = true;
	try {
		await invoke('launch_app', { appId: card.appId });
	}
	catch (e) {
		notify(`起動できませんでした：${e}`);
		btn.disabled = false;
	}
}

async function onUpdateClick(card, btn, node) {
	btn.disabled = true;
	const original = btn.textContent;
	btn.textContent = '処理中…';
	try {
		const outcome = await invoke('install_update', { appId: card.appId });
		if (outcome === 'downloadedOnly') {
			notify('自動での更新ができなかったため、取得したファイルを開きました。表示されたインストーラーの指示に従ってください。');
		}
	}
	catch (e) {
		notify(`更新に失敗しました：${e}`);
	}
	finally {
		btn.disabled = false;
		btn.textContent = original;
	}
}

function onOpenUrlClick(url) {
	return () => invoke('open_url', { url }).catch((e) => notify(`開けませんでした：${e}`));
}

async function onPickLegacyInstallerClick(card, btn) {
	btn.disabled = true;
	try {
		const result = await invoke('pick_legacy_installer', { appId: card.appId });
		if (result === 'cancelled') { /* 何もしない */ }
		else if (result === 'confirmed') {
			notify('確認できました。ただし現在の状態は変わりません。更新がある場合は配布サイトから取得してください。');
		}
		else {
			notify('選んだファイルは、このアプリの過去の正規版として確認できませんでした。');
		}
	}
	catch (e) {
		notify(`確認に失敗しました：${e}`);
	}
	finally {
		btn.disabled = false;
	}
}

function applyCard(node, card) {
	node.dataset.state = card.status;
	node.querySelector('.cardName').textContent = card.appName;

	const versionEl = node.querySelector('.cardVersion');
	versionEl.textContent = card.latestVersion ? `最新: ${card.latestVersion}` : '';

	const icon = node.querySelector('.cardIconImg');
	if (card.iconPngBase64) {
		icon.src = `data:image/png;base64,${card.iconPngBase64}`;
		icon.hidden = false;
	}

	node.querySelector('.spinner').hidden = true;
	const badge = node.querySelector('.statusBadge');
	badge.textContent = STATUS_LABEL[card.status] ?? '';
	badge.className = `statusBadge ${BADGE_CLASS[card.status] ?? ''}`;

	const actions = node.querySelector('.cardActions');
	actions.textContent = '';

	if (CAN_LAUNCH_STATES.has(card.status)) {
		actions.appendChild(makeButton('起動', (e) => onLaunchClick(card, e.target)));
	}
	if (card.status === 'updateAvailable' || card.status === 'archMigration') {
		actions.appendChild(makeButton('更新', (e) => onUpdateClick(card, e.target, node)));
	}
	if (card.status === 'notGenuine') {
		if (card.storeUrl) actions.appendChild(makeButton(card.storeLabel || '購入先を開く', onOpenUrlClick(card.storeUrl)));
		if (manualUrl) actions.appendChild(makeButton('詳しく見る', onOpenUrlClick(manualUrl)));
	}
	if (card.status === 'needsConfirmation') {
		actions.appendChild(makeButton('当時のインストーラーを選ぶ', (e) => onPickLegacyInstallerClick(card, e.target)));
	}
	if (card.status === 'notInstalled' && card.storeUrl) {
		actions.appendChild(makeButton(card.storeLabel || '配布サイトを開く', onOpenUrlClick(card.storeUrl)));
	}
	if (card.status === 'manifestError' || card.status === 'manifestErrorInstalled') {
		actions.appendChild(makeButton('再試行', () => invoke('refresh').catch((e) => notify(`再試行に失敗：${e}`))));
	}
}

function renderCard(card) {
	const tmpl = $('tmplCard');
	const node = tmpl.content.firstElementChild.cloneNode(true);
	applyCard(node, card);
	return node;
}

function placeCard(card) {
	const installed = LOCAL_INSTALL_STATES.has(card.status);
	const container = installed ? $('installedCards') : $('notInstalledCards');
	const section = installed ? $('installedSection') : $('notInstalledSection');

	let node = cardNodes.get(card.appId);
	if (node) {
		applyCard(node, card);
		if (node.parentElement !== container) container.appendChild(node);
	}
	else {
		node = renderCard(card);
		cardNodes.set(card.appId, node);
		container.appendChild(node);
	}
	section.hidden = container.children.length === 0;
}

function renderCircleFooter(view) {
	if (!view.circleName) return;
	$('circleFooter').hidden = false;
	$('circleName').textContent = view.circleName;

	const links = [];
	if (view.circleSiteUrl) links.push(['公式サイト', view.circleSiteUrl]);
	if (view.circleSupportUrl) links.push(['不具合の報告', view.circleSupportUrl]);
	if (view.circleEmail) links.push([view.circleEmail, `mailto:${view.circleEmail}`]);

	const nav = $('circleLinks');
	nav.textContent = '';
	for (const [label, url] of links) {
		const a = el('a', label);
		a.href = '#';
		a.addEventListener('click', (e) => { e.preventDefault(); invoke('open_url', { url }); });
		nav.appendChild(a);
	}
}

async function loadInitialView() {
	let view;
	try {
		view = await invoke('initial_view');
	}
	catch (e) {
		$('emptyNotice').hidden = false;
		$('emptyNotice').textContent = `起動時の読み込みに失敗しました：${e}`;
		return;
	}

	$('launcherName').textContent = view.launcherName || 'Shelf';
	manualUrl = view.manualUrl || '';
	$('btnManual').addEventListener('click', () => invoke('open_url', { url: view.manualUrl }));
	renderCircleFooter(view);

	if (view.cards.length === 0) {
		$('emptyNotice').hidden = false;
		return;
	}
	for (const card of view.cards) placeCard(card);

	// 起動直後にキャッシュを先に出し、裏でマニフェストを取り直す（オフラインでも画面が出る）
	invoke('refresh').catch((e) => console.warn('refresh failed', e));
}

listen('card-changed', (e) => placeCard(e.payload));

$('btnRefresh').addEventListener('click', () => {
	$('btnRefresh').disabled = true;
	invoke('refresh')
		.catch((e) => notify(`更新確認に失敗しました：${e}`))
		.finally(() => { $('btnRefresh').disabled = false; });
});

loadInitialView();
