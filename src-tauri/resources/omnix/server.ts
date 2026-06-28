import express from "express";
import { createServer as createViteServer } from "vite";
import path from "path";
import { fileURLToPath } from "url";
import cors from "cors";
import multer from "multer";
import { createServer } from "http";
import open from "open";
import { setupWebSockets, dispatchTask, handleHealthCheck, archiveReqIdHistory, purgeReqIdHistory } from "./src/engine/socketHandler.ts";

process.on('uncaughtException', (err) => {
  console.error('💥 Uncaught Exception:', err);
});

process.on('unhandledRejection', (reason, promise) => {
  console.error('💥 Unhandled Rejection at:', promise, 'reason:', reason);
});

const resolvedFilename = typeof __filename !== "undefined" ? __filename : fileURLToPath(import.meta.url);
const resolvedDirname = typeof __dirname !== "undefined" ? __dirname : path.dirname(resolvedFilename);

const PORT = parseInt(process.env.PORT || '9777');
const args = process.argv.slice(2);
const isSilent = args.includes("--silent") || process.env.NODE_ENV === "production" || !!process.env.K_SERVICE;
const pidArgIndex = args.indexOf("--dependent-pid");
const dependentPid = pidArgIndex !== -1 ? parseInt(args[pidArgIndex + 1]) : null;

const upload = multer({ storage: multer.memoryStorage() });

async function startServer() {
  console.log("🚀 Initializing Omnix Brain...");
  const app = express();
  const server = createServer(app);
  
  // Initialize WebSockets (Relay Mode) - ONLY IF EXPLICITLY ENABLED OR IN CERTAIN ENVIRONMENTS
  const isElectron = !!process.versions.electron;
  let relayActive = false;

  const startRelay = () => {
    if (relayActive) return;
    console.log("📡 Setting up WebSockets (Relay Mode Enabled)...");
    setupWebSockets(server);
    relayActive = true;
  };

  // Always start the relay so a compute worker (Electron renderer or browser
  // tab) can register over WebSocket. The original gate only enabled it under
  // Electron-forked node, which left system-node launches with no worker
  // endpoint ("No compute worker connected").
  if (isElectron) {
    console.log("Omnix: Running in Electron mode. Auto-launching relay server...");
  } else {
    console.log("Omnix: Running in Standalone mode. Enabling relay for local compute workers...");
  }
  startRelay();

  app.use((req, res, next) => {
    const origin = req.headers.origin || "*";
    res.setHeader("Access-Control-Allow-Origin", origin);
    res.setHeader("Access-Control-Allow-Methods", "GET, POST, OPTIONS, PUT, DELETE, PATCH, HEAD");
    res.setHeader("Access-Control-Allow-Headers", req.headers["access-control-request-headers"] || "Content-Type, Authorization, X-Requested-With, x-req-id, reqid, reqId, Accept");
    res.setHeader("Access-Control-Allow-Credentials", "true");
    
    if (req.headers["access-control-request-private-network"]) {
      res.setHeader("Access-Control-Allow-Private-Network", "true");
    }
    
    if (req.method === "OPTIONS") {
      res.setHeader("Access-Control-Allow-Private-Network", "true");
      return res.status(200).end(); // Respond immediately to preflight OPTIONS requests to bypass CORS
    }
    next();
  });
  // Skip the global body parsers for multipart/form-data so multer (busboy)
  // receives the raw stream. Without this, express.text({type:'*/*'}) drains
  // the multipart body first -> "Unexpected end of form" on /vision and /stt.
  const skipMultipart = (parser: any) => (req: any, res: any, next: any) => {
    const ct = req.headers["content-type"] || "";
    if (ct.includes("multipart/form-data")) return next();
    return parser(req, res, next);
  };
  app.use(skipMultipart(express.json({ limit: '50mb' })));
  app.use(skipMultipart(express.urlencoded({ extended: true, limit: '50mb' })));
  app.use(skipMultipart(express.text({ type: '*/*', limit: '50mb' })));

  // Normalization middleware to parse stringified JSON or urlencoded JSON from PowerShell/clients
  app.use((req, res, next) => {
    if (typeof req.body === "string") {
      const trimmed = req.body.trim();
      if (trimmed.startsWith("{") || trimmed.startsWith("[")) {
        try {
          req.body = JSON.parse(trimmed);
        } catch (e) {
          // Leave as raw string
        }
      } else {
        try {
          const params = new URLSearchParams(trimmed);
          const obj: any = {};
          let hasKeys = false;
          for (const [key, val] of params.entries()) {
            obj[key] = val;
            hasKeys = true;
          }
          if (hasKeys) {
            const keys = Object.keys(obj);
            if (keys.length === 1 && keys[0].trim().startsWith("{")) {
              try {
                req.body = JSON.parse(keys[0]);
              } catch {
                req.body = obj;
              }
            } else {
              req.body = obj;
            }
          }
        } catch (e) {
          // Leave as raw string
        }
      }
    }
    next();
  });

  // Relay Control
  app.post("/api/server/relay", (req, res) => {
    const { action } = req.body;
    if (action === "start") {
      startRelay();
      return res.json({ status: "ok", message: "Relay started" });
    }
    res.json({ status: "ok", relayActive });
  });

  app.get("/api/server/status", (req, res) => {
    res.json({ 
      relayActive, 
      isElectron, 
      platform: process.platform,
      arch: process.arch
    });
  });

  // Health check
  app.get("/api/health", async (req, res) => {
    try {
      const origin = req.headers.origin || req.headers.referer || "unknown";
      const check = await handleHealthCheck(origin);
      if (!check.allowed) {
        return res.status(403).json({ error: check.error || "Blocked by permission policy" });
      }
      res.json({ status: "ok", pid: process.pid });
    } catch (err: any) {
      res.status(500).json({ error: err.message });
    }
  });

  // --- 2. PID MONITORING ---
  if (dependentPid && !isNaN(dependentPid)) {
    console.log(`Omnix: Monitoring parent process PID ${dependentPid}`);
    setInterval(() => {
      try {
        process.kill(dependentPid, 0);
      } catch (e) {
        console.log("Omnix: Parent process ended or unreachable. Shutting down...");
        process.exit();
      }
    }, 2000);
  } else if (pidArgIndex !== -1) {
    console.log("Omnix: --dependent-pid provided but invalid. Skipping monitoring.");
  }

  // --- API ENDPOINTS (Relayed to Browser Engine) ---
  
  // Text Generation
  app.post("/api/text", async (req, res) => {
    try {
      const { prompt, systemPrompt, modelId, temperature, top_p, maxTokens } = req.body;
      if (!prompt) return res.status(400).json({ error: "Prompt is required" });

      const origin = req.headers.origin || req.headers.referer || "unknown";
      const reqId = req.body?.reqId || req.query?.reqId || req.headers?.["x-req-id"] || req.headers?.["reqid"];
      const output = await dispatchTask("text", prompt, { systemPrompt, modelId, origin, reqId, temperature, top_p, maxTokens });
      
      let cleanResponse = output;
      let thinkText: string | undefined = undefined;
      
      if (typeof output === "string") {
        const thinkRegex = /<think>([\s\S]*?)(?:<\/think>|$)/i;
        const match = output.match(thinkRegex);
        if (match) {
          thinkText = match[1].trim();
          cleanResponse = output.replace(thinkRegex, "").trim();
        } else {
          const thoughtRegex = /<\|channel>thought\n([\s\S]*?)(?:<channel\|>|$)/i;
          const thoughtMatch = output.match(thoughtRegex);
          if (thoughtMatch) {
            thinkText = thoughtMatch[1].trim();
            cleanResponse = output.replace(thoughtRegex, "").trim();
          }
        }
      }

      if (thinkText !== undefined) {
        res.json({ response: cleanResponse, think: thinkText });
      } else {
        res.json({ response: output });
      }
    } catch (error: any) {
      res.status(500).json({ error: error.message });
    }
  });

  // Director Routing
  app.post("/api/director", async (req, res) => {
    try {
      const { prompt } = req.body;
      if (!prompt) return res.status(400).json({ error: "Prompt is required" });

      const origin = req.headers.origin || req.headers.referer || "unknown";
      const reqId = req.body?.reqId || req.query?.reqId || req.headers?.["x-req-id"] || req.headers?.["reqid"];
      const intent = await dispatchTask("director", prompt, { origin, reqId });
      res.json({ intent, prompt });
    } catch (error: any) {
      res.status(500).json({ error: error.message });
    }
  });

  // Archive isolated chat history for a reqId
  app.post("/api/archive-history", (req, res) => {
    const { oldReqId, newReqId } = req.body;
    if (oldReqId !== undefined && newReqId !== undefined) {
      archiveReqIdHistory(String(oldReqId), String(newReqId));
      return res.json({ success: true, message: `Archived ${oldReqId} to ${newReqId}` });
    }
    return res.status(400).json({ error: "oldReqId and newReqId are required" });
  });

  // Purge isolated chat history for a reqId
  app.post("/api/purge-history", (req, res) => {
    const { reqId } = req.body;
    if (reqId !== undefined) {
      purgeReqIdHistory(String(reqId));
      return res.json({ success: true, message: `Purged reqId: ${reqId}` });
    }
    return res.status(400).json({ error: "reqId is required" });
  });

  // Vision Analysis
  app.post("/api/vision", upload.single("image"), async (req: any, res) => {
    try {
      const { prompt, modelId } = req.body;
      const file = req.file;
      if (!file) return res.status(400).json({ error: "Image is required" });

      const origin = req.headers.origin || req.headers.referer || "unknown";
      const reqId = req.body?.reqId || req.query?.reqId || req.headers?.["x-req-id"] || req.headers?.["reqid"];
      const base64Image = `data:image/jpeg;base64,${file.buffer.toString('base64')}`;
      const response = await dispatchTask("vision", base64Image, { prompt, modelId, origin, reqId });

      let cleanResponse = response;
      let thinkText: string | undefined = undefined;
      
      if (typeof response === "string") {
        const thinkRegex = /<think>([\s\S]*?)(?:<\/think>|$)/i;
        const match = response.match(thinkRegex);
        if (match) {
          thinkText = match[1].trim();
          cleanResponse = response.replace(thinkRegex, "").trim();
        } else {
          const thoughtRegex = /<\|channel>thought\n([\s\S]*?)(?:<channel\|>|$)/i;
          const thoughtMatch = response.match(thoughtRegex);
          if (thoughtMatch) {
            thinkText = thoughtMatch[1].trim();
            cleanResponse = response.replace(thoughtRegex, "").trim();
          }
        }
      }

      if (thinkText !== undefined) {
        res.json({ response: cleanResponse, think: thinkText });
      } else {
        res.json({ response });
      }
    } catch (error: any) {
      res.status(500).json({ error: error.message });
    }
  });

  // Speech-to-Text
  app.post("/api/stt", upload.single("audio"), async (req: any, res) => {
    try {
      const file = req.file;
      if (!file) return res.status(400).json({ error: "Audio file is required" });
      
      const origin = req.headers.origin || req.headers.referer || "unknown";
      const reqId = req.body?.reqId || req.query?.reqId || req.headers?.["x-req-id"] || req.headers?.["reqid"];
      const base64Audio = file.buffer.toString('base64');
      const text = await dispatchTask("stt", base64Audio, { origin, reqId });

      res.json({ text });
    } catch (error: any) {
      res.status(500).json({ error: error.message });
    }
  });

  // Text-to-Speech
  app.post("/api/tts", async (req, res) => {
    try {
      const { text, voiceID, voiceId, modelId } = req.body;
      if (!text) return res.status(400).json({ error: "Text is required" });

      const origin = req.headers.origin || req.headers.referer || "unknown";
      const reqId = req.body?.reqId || req.query?.reqId || req.headers?.["x-req-id"] || req.headers?.["reqid"];
      
      const selectedVoice = voiceID || voiceId || modelId || "af_heart";

      const output = await dispatchTask("tts", text, { voiceID: selectedVoice, origin, reqId });
      res.json(output);
    } catch (error: any) {
      res.status(500).json({ error: error.message });
    }
  });

  // Image Generation
  app.post("/api/image", async (req, res) => {
    try {
      const { prompt, modelId } = req.body;
      if (!prompt) return res.status(400).json({ error: "Prompt is required" });

      const origin = req.headers.origin || req.headers.referer || "unknown";
      const reqId = req.body?.reqId || req.query?.reqId || req.headers?.["x-req-id"] || req.headers?.["reqid"];
      const image = await dispatchTask("image-gen", prompt, { modelId, origin, reqId });
      res.json({ status: "success", image });
    } catch (error: any) {
      res.status(500).json({ error: error.message });
    }
  });

  // Music Generation
  app.post("/api/music", async (req, res) => {
    try {
      const { prompt, modelId } = req.body;
      if (!prompt) return res.status(400).json({ error: "Prompt is required" });

      const origin = req.headers.origin || req.headers.referer || "unknown";
      const reqId = req.body?.reqId || req.query?.reqId || req.headers?.["x-req-id"] || req.headers?.["reqid"];
      const output = await dispatchTask("music-gen", prompt, { modelId, origin, reqId });
      res.json({ status: "success", ...output });
    } catch (error: any) {
      res.status(500).json({ error: error.message });
    }
  });

  // --- FRONTEND MIDDLEWARE ---
  if (process.env.NODE_ENV !== "production") {
    console.log("🛠️ Starting Vite in development mode...");
    const vite = await createViteServer({
      server: { middlewareMode: true },
      appType: "spa",
    });
    app.use(vite.middlewares);
  } else {
    console.log("📦 Serving production build...");
    let distPath: string;
    if (resolvedFilename.endsWith("server.cjs")) {
      distPath = resolvedDirname;
    } else {
      distPath = path.join(resolvedDirname, "dist");
    }
    app.use(express.static(distPath));
    app.get("*", (req, res) => {
      res.sendFile(path.join(distPath, "index.html"));
    });
  }

  server.on("error", (err: any) => {
    if (err.code === "EADDRINUSE") {
      console.error(`❌ Port ${PORT} is already in use. Please close the other process or set a different PORT.`);
    } else {
      console.error("❌ Server Error:", err);
    }
  });

  server.listen(PORT, "0.0.0.0", async () => {
    console.log(`🚀 Omnix Brain Active [PID: ${process.pid}] on port ${PORT}`);
    console.log(`🤖 Local API available at http://localhost:${PORT}/api`);
    
    // Only launch GUI if we are definitely local and not silent
    const isLocal = !process.env.K_SERVICE && !process.env.GAE_SERVICE; // Cloud Run / App Engine check
    if (!isSilent && isLocal) {
      launchGUI();
    }
  });
}

async function launchGUI() {
  try {
    const { default: electron } = await import("electron" as any).catch(() => ({ default: null }));
    
    if (electron && (electron as any).app) {
      console.log("Omnix: Handled by Electron shell.");
    } else {
      console.log("Omnix: Opening GUI in default browser...");
      await open(`http://localhost:${PORT}`);
    }
  } catch (err) {
    console.log("Omnix: Error launching GUI, opening in default browser...");
    await open(`http://localhost:${PORT}`);
  }
}

startServer().catch((err) => {
  console.error("Critical: Omnix Brain failed to start:", err);
  process.exit(1);
});
