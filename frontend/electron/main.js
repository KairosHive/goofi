// The app shell: it runs the goofi backend beside it, shows the app it serves, and installs a
// newer release when the backend asks (`goofi update`) or one is found at start.
const { app, BrowserWindow, dialog, shell } = require('electron');
const { spawn } = require('node:child_process');
const path = require('node:path');
const { autoUpdater } = require('electron-updater');
const WebSocket = require('ws');

const RELEASES = 'https://github.com/KairosHive/goofi/releases';
const exe = process.platform === 'win32' ? 'goofi.exe' : 'goofi';

/** The backend: beside the shell's resources when packaged, the checkout's debug build otherwise. */
function backendPath() {
	if (app.isPackaged) return path.join(process.resourcesPath, 'bin', exe);
	return path.join(__dirname, '..', '..', 'target', 'debug', exe);
}

let backend = null;
let window = null;
let tail = [];

/** Start goofi and resolve with the URL it prints; reject with its last lines if it ends first. */
function startBackend() {
	return new Promise((resolve, reject) => {
		// `--shell`: the backend ends when this stdin closes, so a dead shell leaves no server.
		backend = spawn(backendPath(), ['--shell', '--port', '0'], { stdio: ['pipe', 'pipe', 'pipe'] });
		let url = null;
		const read = (chunk) => {
			for (const line of chunk.toString().split('\n')) {
				if (!line) continue;
				tail = [...tail, line].slice(-40);
				const found = line.match(/goofi → (http:\/\/\S+)/);
				if (found && !url) {
					url = found[1];
					resolve(url);
				}
			}
		};
		backend.stdout.on('data', read);
		backend.stderr.on('data', read);
		backend.on('exit', (code) => {
			if (!url) reject(new Error(`goofi exited with code ${code}:\n${tail.join('\n')}`));
			else if (!app.isQuiting) {
				dialog.showErrorBox('goofi stopped', tail.join('\n'));
				app.quit();
			}
		});
		backend.on('error', (error) => reject(error));
	});
}

/** The control socket, for the one event the shell answers: an update request. */
function listen(url) {
	const socket = new WebSocket(`${url.replace(/^http/, 'ws')}/control?actor=shell`);
	socket.on('message', (data) => {
		let message;
		try { message = JSON.parse(data.toString()); } catch { return; }
		if (message.event === 'update_requested') update();
	});
	socket.on('close', () => setTimeout(() => backend && !app.isQuiting && listen(url), 1000));
	socket.on('error', () => {});
}

/** Download the newest release and restart into it; where the updater cannot, open the releases. */
async function update() {
	if (!app.isPackaged) {
		dialog.showMessageBox({ message: 'Not packaged: an update applies to an installed goofi.' });
		return;
	}
	try {
		const result = await autoUpdater.checkForUpdates();
		if (!result || !result.updateInfo || result.updateInfo.version === app.getVersion()) {
			dialog.showMessageBox({ message: `goofi ${app.getVersion()} is the newest release.` });
		}
	} catch (error) {
		const answer = await dialog.showMessageBox({
			type: 'warning',
			message: `The update could not be installed from here: ${error.message}`,
			buttons: ['Open the releases page', 'Close'],
		});
		if (answer.response === 0) shell.openExternal(RELEASES);
	}
}

autoUpdater.autoDownload = true;
autoUpdater.on('update-downloaded', () => {
	app.isQuiting = true;
	autoUpdater.quitAndInstall();
});

app.whenReady().then(async () => {
	let url;
	try {
		url = await startBackend();
	} catch (error) {
		dialog.showErrorBox('goofi could not start', error.message);
		app.quit();
		return;
	}
	window = new BrowserWindow({
		width: 1400,
		height: 900,
		title: 'goofi',
		backgroundColor: '#101014',
		webPreferences: { contextIsolation: true, sandbox: true },
	});
	window.on('closed', () => { window = null; app.quit(); });
	await window.loadURL(url);
	listen(url);
	if (app.isPackaged) autoUpdater.checkForUpdatesAndNotify().catch(() => {});
});

app.on('before-quit', () => {
	app.isQuiting = true;
	if (backend) backend.stdin.end();
});
app.on('window-all-closed', () => app.quit());
