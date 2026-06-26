// Meridian Downloader - background service worker (MV3).
// Owns the cross-origin POST to the Meridian receiver so the content script
// never has to make the request itself. The content script detects a media URL
// and asks the worker to forward it.

const MERIDIAN_ENDPOINT = 'http://localhost:7771/download';
const MERIDIAN_PING = 'http://localhost:7771/ping';

async function forwardDownload(payload) {
  const response = await fetch(MERIDIAN_ENDPOINT, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(payload),
  });
  if (!response.ok) {
    throw new Error(`Meridian responded ${response.status}`);
  }
  return true;
}

async function pingMeridian() {
  try {
    const response = await fetch(MERIDIAN_PING, { method: 'POST' });
    return response.ok;
  }
  catch {
    return false;
  }
}

chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  if (message && message.action === 'ping') {
    void pingMeridian().then(online => sendResponse({ status: online ? 'ok' : 'offline' }));
    return true;
  }
  if (message && message.action === 'download' && message.payload) {
    forwardDownload(message.payload)
      .then(() => sendResponse({ status: 'ok' }))
      .catch(error => sendResponse({ status: 'error', error: String(error) }));
    return true;
  }
  return false;
});
