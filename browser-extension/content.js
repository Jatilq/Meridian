(function () {
  const MERIDIAN_ENDPOINT = 'http://localhost:7771/download';
  const VIDEO_EXTENSIONS = ['.mp4', '.webm', '.mkv', '.avi', '.mov', '.wmv', '.flv', '.m4v', '.mpg', '.mpeg', '.m3u8', '.mpd', '.ts'];
  const BUTTON_SIZE = 36;

  let floatingButton = null;
  let visible = false;

  function createButton() {
    if (floatingButton) return;
    floatingButton = document.createElement('div');
    floatingButton.id = 'meridian-dl-float';
    floatingButton.title = 'Download with Meridian';
    floatingButton.innerHTML = '⬇';
    floatingButton.addEventListener('click', onButtonClick);
    document.body.appendChild(floatingButton);
  }

  function showButton() {
    if (visible) return;
    createButton();
    floatingButton.classList.add('meridian-dl-visible');
    visible = true;
  }

  function hideButton() {
    if (!visible || !floatingButton) return;
    floatingButton.classList.remove('meridian-dl-visible');
    visible = false;
  }

  async function onButtonClick() {
    const url = detectBestUrl();
    if (!url) {
      alert('No downloadable video URL detected on this page.');
      return;
    }

    const payload = {
      url,
      fileName: guessFileName(url),
      formatId: null,
      autoSaveFolder: null,
    };

    try {
      const result = await chrome.runtime.sendMessage({ action: 'download', payload });
      if (!result || result.status !== 'ok') {
        alert('Failed to reach Meridian. Is the app running?');
      }
    }
    catch {
      alert('Failed to reach Meridian. Is the app running?');
    }
  }

  function detectBestUrl() {
    const videos = Array.from(document.querySelectorAll('video'));
    for (const video of videos) {
      const src = video.currentSrc || video.src;
      if (src && isVideoLike(src)) return src;
    }
    for (const a of Array.from(document.querySelectorAll('a[href]'))) {
      const href = a.getAttribute('href');
      if (href && isVideoLike(href)) return new URL(href, window.location.href).href;
    }
    for (const source of Array.from(document.querySelectorAll('source[src]'))) {
      const src = source.getAttribute('src');
      if (src && isVideoLike(src)) return new URL(src, window.location.href).href;
    }
    return null;
  }

  function isVideoLike(url) {
    try {
      const u = new URL(url, window.location.href);
      const path = u.pathname.toLowerCase();
      return VIDEO_EXTENSIONS.some(ext => path.endsWith(ext)) || u.hostname.includes('youtube') || u.hostname.includes('youtu.be');
    }
    catch {
      return false;
    }
  }

  function guessFileName(url) {
    try {
      const u = new URL(url, window.location.href);
      const parts = u.pathname.split('/');
      const name = parts.pop() || 'download';
      return name.split('?')[0] || 'download';
    }
    catch {
      return 'download';
    }
  }

  function observe() {
    const videos = document.querySelectorAll('video');
    if (videos.length > 0) {
      showButton();
    } else {
      hideButton();
    }
  }

  const observer = new MutationObserver(observe);
  observer.observe(document.body || document.documentElement, { childList: true, subtree: true });

  observe();
})();
