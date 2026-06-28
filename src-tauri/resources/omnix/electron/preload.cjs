const { contextBridge, ipcRenderer } = require('electron');

contextBridge.exposeInMainWorld('electron', {
  fs: {
    readDir: (dirPath) => ipcRenderer.invoke('fs:readDir', dirPath),
    readFile: (filePath) => ipcRenderer.invoke('fs:readFile', filePath),
    writeFile: (filePath, content) => ipcRenderer.invoke('fs:writeFile', filePath, content),
  },
  os: {
    getMemoryStats: () => ipcRenderer.invoke('os:getMemoryStats')
  },
  server: {
    start: () => ipcRenderer.invoke('server:start'),
    stop: () => ipcRenderer.invoke('server:stop'),
    isStarted: () => ipcRenderer.invoke('server:isStarted'),
    getPort: () => ipcRenderer.sendSync('server:getPortSync'),
  },
  dialog: {
    openFile: () => ipcRenderer.invoke('dialog:openFile'),
  },
  ipcRenderer: {
    send: (channel, ...args) => ipcRenderer.send(channel, ...args)
  },
  onServerLog: (callback) => {
    const listener = (event, data) => callback(data);
    ipcRenderer.on('server:log', listener);
    return () => ipcRenderer.removeListener('server:log', listener);
  },
  onInferenceRequest: (callback) => ipcRenderer.on('execute-inference', (event, data) => callback(data)),
  sendInferenceResult: (requestId, response) => ipcRenderer.send('inference-result', { requestId, response })
});
