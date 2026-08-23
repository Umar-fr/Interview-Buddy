import { getCurrentWindow } from "@tauri-apps/api/window";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

const appWindow = getCurrentWindow();

// ==========================================
// Window Controls
// ==========================================
document
  .getElementById("minimize")
  ?.addEventListener("click", async (event) => {
    event.stopPropagation();
    try {
      await invoke("minimize_app");
    } catch {
      await appWindow.minimize();
    }
  });

document
  .getElementById("close")
  ?.addEventListener("click", async (event) => {
    event.stopPropagation();
    try {
      await invoke("close_app");
    } catch {
      await appWindow.close();
    }
  });

// ==========================================
// DOM Elements
// ==========================================
const audioBadge = document.getElementById("audio-badge");
const audioStatusLabel = document.getElementById("audio-status-label");
const audioMeterBar = document.getElementById("audio-meter-bar");
const statusDetail = document.getElementById("status-detail");
const modelStatus = document.getElementById("model-status");
const aiBadgeBtn = document.getElementById("ai-settings-btn");
const aiBadgeLabel = document.getElementById("ai-badge-label");

// Transcript Elements
const transcriptBox = document.getElementById("transcript");
const placeholder = document.getElementById("placeholder");
const copyBtn = document.getElementById("copy-btn");
const clearBtn = document.getElementById("clear-btn");

// AI Copilot Elements
const aiAnswersBox = document.getElementById("ai-answers");
const aiPlaceholder = document.getElementById("ai-placeholder");
const clearAiBtn = document.getElementById("clear-ai-btn");
const autoAnswerCheckbox = document.getElementById("auto-answer-checkbox") as HTMLInputElement | null;

// Modal Elements
const modalOverlay = document.getElementById("api-key-modal");
const modalCloseBtn = document.getElementById("modal-close-btn");
const apiKeyInput = document.getElementById("api-key-input") as HTMLInputElement | null;
const modelSelect = document.getElementById("model-select") as HTMLSelectElement | null;
const saveKeyBtn = document.getElementById("save-key-btn");
const saveKeyBtnText = document.getElementById("save-key-btn-text");
const toggleKeyVisibilityBtn = document.getElementById("toggle-key-visibility");
const modalStatusMsg = document.getElementById("modal-status-msg");

const refreshModelsBtn = document.getElementById("refresh-models-btn") as HTMLButtonElement | null;
const modelDetectHint = document.getElementById("model-detect-hint");
const rememberKeyCheckbox = document.getElementById("remember-key-checkbox") as HTMLInputElement | null;

// ==========================================
// Gemini AI State & Storage
// ==========================================
const STORAGE_KEY_API_KEY = "gemini_api_key";
const STORAGE_KEY_MODEL = "gemini_model";

let geminiApiKey: string = localStorage.getItem(STORAGE_KEY_API_KEY) || "";
let selectedModel: string = localStorage.getItem(STORAGE_KEY_MODEL) || "";

function updateAiBadgeState() {
  if (aiBadgeBtn && aiBadgeLabel) {
    if (geminiApiKey && selectedModel) {
      aiBadgeBtn.classList.add("configured");
      aiBadgeLabel.textContent = "Gemini Active";
    } else {
      aiBadgeBtn.classList.remove("configured");
      aiBadgeLabel.textContent = "Set API Key";
    }
  }
}

// Show API key setup modal
function openApiKeyModal() {
  if (modalOverlay) {
    modalOverlay.classList.remove("hidden");
    if (apiKeyInput) {
      apiKeyInput.value = geminiApiKey;
      apiKeyInput.focus();
    }
    if (rememberKeyCheckbox) {
      // Checked only if previously saved in storage; unchecked by default for new sessions
      rememberKeyCheckbox.checked = Boolean(localStorage.getItem(STORAGE_KEY_API_KEY));
    }
    if (modalStatusMsg) {
      modalStatusMsg.className = "modal-status-msg hidden";
      modalStatusMsg.textContent = "";
    }

    if (geminiApiKey) {
      detectAndPopulateModels(geminiApiKey, false);
    } else if (modelSelect) {
      modelSelect.innerHTML = '<option value="" disabled selected>Enter your API key above to detect models...</option>';
      modelSelect.disabled = true;
      if (modelDetectHint) {
        modelDetectHint.textContent = "Available models will be fetched live from your Gemini API key.";
        modelDetectHint.style.color = "";
      }
    }
  }
}

function closeApiKeyModal() {
  if (modalOverlay) {
    modalOverlay.classList.add("hidden");
  }
}

// Event Listeners for Modal
aiBadgeBtn?.addEventListener("click", () => {
  openApiKeyModal();
});

modalCloseBtn?.addEventListener("click", () => {
  closeApiKeyModal();
});

modalOverlay?.addEventListener("click", (e) => {
  if (e.target === modalOverlay) {
    closeApiKeyModal();
  }
});

toggleKeyVisibilityBtn?.addEventListener("click", () => {
  if (apiKeyInput) {
    apiKeyInput.type = apiKeyInput.type === "password" ? "text" : "password";
  }
});

document.getElementById("open-ai-studio-link")?.addEventListener("click", async (e) => {
  e.preventDefault();
  try {
    const { openUrl } = await import("@tauri-apps/plugin-opener");
    await openUrl("https://aistudio.google.com/app/apikey");
  } catch {
    window.open("https://aistudio.google.com/app/apikey", "_blank");
  }
});

// Dynamic API detection & user model selection prompt
let detectDebounceTimer: number | null = null;

apiKeyInput?.addEventListener("input", () => {
  if (detectDebounceTimer) clearTimeout(detectDebounceTimer);
  detectDebounceTimer = window.setTimeout(() => {
    const key = apiKeyInput.value.trim();
    if (key.length >= 20) {
      detectAndPopulateModels(key, false);
    }
  }, 500);
});

apiKeyInput?.addEventListener("change", () => {
  const key = apiKeyInput.value.trim();
  if (key.length >= 20) {
    detectAndPopulateModels(key, false);
  }
});

refreshModelsBtn?.addEventListener("click", () => {
  const key = apiKeyInput?.value.trim() || geminiApiKey;
  if (key) {
    detectAndPopulateModels(key, true);
  } else {
    showModalStatus("Please enter an API key first.", "error");
  }
});

modelSelect?.addEventListener("change", () => {
  if (modelSelect.value) {
    selectedModel = modelSelect.value;
  }
});

interface DetectedModel {
  id: string;
  label: string;
}

async function detectAndPopulateModels(key: string, forcePrompt = false): Promise<DetectedModel[]> {
  if (!key) {
    if (modelSelect) {
      modelSelect.innerHTML = '<option value="" disabled selected>Enter your API key above to detect models...</option>';
      modelSelect.disabled = true;
    }
    return [];
  }

  if (refreshModelsBtn) {
    refreshModelsBtn.disabled = true;
    refreshModelsBtn.textContent = "Detecting...";
  }

  if (modelSelect) {
    modelSelect.innerHTML = '<option value="" disabled selected>⏳ Querying Gemini API for available models...</option>';
    modelSelect.disabled = true;
  }

  if (modelDetectHint) {
    modelDetectHint.textContent = "Fetching supported models for your account...";
    modelDetectHint.style.color = "";
  }

  try {
    const listUrl = `https://generativelanguage.googleapis.com/v1beta/models?key=${encodeURIComponent(key)}`;
    const res = await fetch(listUrl);
    const data = await res.json().catch(() => ({}));

    if (!res.ok) {
      const errMsg = data?.error?.message || `HTTP ${res.status}: Failed to fetch models`;
      if (modelSelect) {
        modelSelect.innerHTML = '<option value="" disabled selected>⚠️ Failed to load models (check API key)</option>';
        modelSelect.disabled = true;
      }
      if (modelDetectHint) {
        modelDetectHint.textContent = `Error: ${errMsg}`;
        modelDetectHint.style.color = "#f87171";
      }
      return [];
    }

    const rawModels = data.models || [];
    const validModels: DetectedModel[] = rawModels
      .filter((m: any) => m.supportedGenerationMethods?.includes("generateContent"))
      .filter((m: any) => {
        const name = (m.name || "").toLowerCase();
        // Filter out embeddings or non-generative endpoints
        return !name.includes("embedding") && !name.includes("aqa") && !name.includes("2.5-flash");
      })
      .map((m: any) => {
        const cleanId = m.name.replace(/^models\//, "");
        const displayName = m.displayName ? `${m.displayName} (${cleanId})` : cleanId;
        return {
          id: cleanId,
          label: displayName,
        };
      });

    if (validModels.length === 0) {
      if (modelSelect) {
        modelSelect.innerHTML = '<option value="" disabled selected>No generateContent models found for this key</option>';
        modelSelect.disabled = true;
      }
      if (modelDetectHint) {
        modelDetectHint.textContent = "No content generation models found for this API key.";
        modelDetectHint.style.color = "#f87171";
      }
      return [];
    }

    if (modelSelect) {
      modelSelect.innerHTML = "";

      // Add a prompt option
      const defaultOption = document.createElement("option");
      defaultOption.value = "";
      defaultOption.disabled = true;
      defaultOption.textContent = `-- Select a model (${validModels.length} detected) --`;
      modelSelect.appendChild(defaultOption);

      validModels.forEach((m) => {
        const opt = document.createElement("option");
        opt.value = m.id;
        opt.textContent = m.label;
        modelSelect.appendChild(opt);
      });

      modelSelect.disabled = false;

      // Select previous selection if present in the detected list and not forcing re-prompt
      const existingMatch = validModels.find((m) => m.id === selectedModel);
      if (existingMatch && !forcePrompt) {
        modelSelect.value = selectedModel;
        defaultOption.selected = false;
      } else {
        defaultOption.selected = true;
        modelSelect.value = "";
      }
    }

    if (modelDetectHint) {
      modelDetectHint.textContent = `✓ Detected ${validModels.length} models from your API key. Please select one.`;
      modelDetectHint.style.color = "#34d399";
    }

    return validModels;
  } catch (err: any) {
    if (modelSelect) {
      modelSelect.innerHTML = '<option value="" disabled selected>⚠️ Error loading models</option>';
      modelSelect.disabled = true;
    }
    if (modelDetectHint) {
      modelDetectHint.textContent = `Connection error: ${err.message || err}`;
      modelDetectHint.style.color = "#f87171";
    }
    return [];
  } finally {
    if (refreshModelsBtn) {
      refreshModelsBtn.disabled = false;
      refreshModelsBtn.textContent = "⟳ Detect Models";
    }
  }
}

// Test and Save API Key
saveKeyBtn?.addEventListener("click", async () => {
  const inputKey = apiKeyInput?.value.trim() || "";
  const chosenModel = modelSelect?.value;

  if (!inputKey) {
    showModalStatus("Please enter your Gemini API Key (starts with AIza...)", "error");
    return;
  }

  if (!chosenModel) {
    showModalStatus("Please select an available model from the dropdown above.", "error");
    return;
  }

  if (saveKeyBtnText) saveKeyBtnText.textContent = "Testing model connection...";
  if (saveKeyBtn) (saveKeyBtn as HTMLButtonElement).disabled = true;

  try {
    const result = await testGeminiApiKey(inputKey, chosenModel);
    if (result.ok) {
      geminiApiKey = inputKey;
      selectedModel = chosenModel;

      if (rememberKeyCheckbox?.checked) {
        localStorage.setItem(STORAGE_KEY_API_KEY, geminiApiKey);
        localStorage.setItem(STORAGE_KEY_MODEL, selectedModel);
      } else {
        localStorage.removeItem(STORAGE_KEY_API_KEY);
        localStorage.removeItem(STORAGE_KEY_MODEL);
      }

      updateAiBadgeState();
      const rememberMsg = rememberKeyCheckbox?.checked ? " (saved on device)" : " (session only)";
      showModalStatus(`✓ Connected successfully to ${chosenModel}${rememberMsg}!`, "success");

      setTimeout(() => {
        closeApiKeyModal();
      }, 1000);
    } else {
      showModalStatus(`API Error (${chosenModel}): ${result.error}`, "error");
    }
  } catch (err: any) {
    showModalStatus(`Connection error: ${err.message || err}`, "error");
  } finally {
    if (saveKeyBtnText) saveKeyBtnText.textContent = "Save & Connect";
    if (saveKeyBtn) (saveKeyBtn as HTMLButtonElement).disabled = false;
  }
});

function showModalStatus(msg: string, type: "success" | "error") {
  if (modalStatusMsg) {
    modalStatusMsg.className = `modal-status-msg ${type}`;
    modalStatusMsg.textContent = msg;
  }
}

// Check on initial startup: if no key is saved, prompt user
window.addEventListener("DOMContentLoaded", () => {
  updateAiBadgeState();
  if (!geminiApiKey) {
    setTimeout(() => {
      openApiKeyModal();
    }, 400);
  }
});

// ==========================================
// Gemini API Communication
// ==========================================
async function testGeminiApiKey(key: string, model: string): Promise<{ ok: boolean; error?: string }> {
  try {
    const cleanModel = model.startsWith("models/") ? model : `models/${model}`;
    const url = `https://generativelanguage.googleapis.com/v1beta/${cleanModel}:generateContent?key=${key}`;
    const response = await fetch(url, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        contents: [{ parts: [{ text: "Hello" }] }],
      }),
    });

    if (!response.ok) {
      const errorData = await response.json().catch(() => ({}));
      const message =
        errorData?.error?.message || `HTTP ${response.status} (${response.statusText})`;
      return { ok: false, error: message };
    }

    return { ok: true };
  } catch (err: any) {
    return { ok: false, error: err.message || String(err) };
  }
}

async function requestGeminiAnswer(questionText: string, contextHistory: string): Promise<string> {
  if (!geminiApiKey) {
    throw new Error("No Gemini API Key configured. Click '✦ Set API Key' to configure.");
  }

  if (selectedModel.includes("2.5") || !selectedModel) {
    selectedModel = "gemini-2.0-flash";
    localStorage.setItem(STORAGE_KEY_MODEL, selectedModel);
  }

  const cleanModel = selectedModel.startsWith("models/") ? selectedModel : `models/${selectedModel}`;
  const url = `https://generativelanguage.googleapis.com/v1beta/${cleanModel}:generateContent?key=${geminiApiKey}`;

  const prompt = `You are an expert real-time assistant and copilot listening to live audio (such as a meeting, lecture, quiz, English test, or interview).
Your task is to analyze the recent audio context and directly answer the question or exercise being asked.

Context of recent speech:
"""
${contextHistory}
"""

Current question / problem prompt:
"""
${questionText}
"""

Instructions:
1. Provide the direct, accurate, and concise answer immediately.
2. If this is a multiple-choice question, explicitly state the correct option and letter (e.g. "**Option B: will be arriving**").
3. Keep your explanation to 1-2 short sentences maximum so the user can read it instantly.`;

  const response = await fetch(url, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      contents: [{ parts: [{ text: prompt }] }],
      generationConfig: {
        maxOutputTokens: 250,
        temperature: 0.2,
      },
    }),
  });

  if (!response.ok) {
    const errorBody = await response.json().catch(() => ({}));
    throw new Error(errorBody?.error?.message || `HTTP ${response.status}`);
  }

  const data = await response.json();
  const answerText = data.candidates?.[0]?.content?.parts?.[0]?.text || "No answer generated.";
  return formatAiAnswerHtml(answerText);
}


function formatAiAnswerHtml(raw: string): string {
  // Convert markdown bold to strong and cleanup
  return raw
    .replace(/\*\*(.*?)\*\*/g, "<strong>$1</strong>")
    .replace(/\*(.*?)\*/g, "<em>$1</em>")
    .replace(/\n/g, "<br/>");
}

// ==========================================
// Question Detection & Copilot UI
// ==========================================
let lastAnsweredQuestion = "";
let lastAnsweredTime = 0;

function isQuestionLike(text: string): boolean {
  const clean = text.trim().toLowerCase();
  if (clean.includes("?")) return true;

  // Common spoken quiz / test / query patterns
  const questionPatterns = [
    /which one/i,
    /what is/i,
    /what was/i,
    /what are/i,
    /why is/i,
    /why did/i,
    /how do/i,
    /how many/i,
    /choose the/i,
    /select the/i,
    /should it be/i,
    /correct option/i,
    /question was/i,
    /number (one|two|three|four|five|1|2|3|4|5)/i,
    /true or false/i,
  ];

  return questionPatterns.some((pattern) => pattern.test(clean));
}

function triggerAiQuestionAnswer(questionText: string, isManual = false) {
  if (!geminiApiKey) {
    if (isManual) openApiKeyModal();
    return;
  }

  const now = Date.now();
  // Prevent duplicate query spamming within 8 seconds for the same text
  if (!isManual && textSimilarity(lastAnsweredQuestion, questionText) > 0.7 && now - lastAnsweredTime < 8000) {
    return;
  }

  lastAnsweredQuestion = questionText;
  lastAnsweredTime = now;

  // Remove AI placeholder
  if (aiPlaceholder && aiPlaceholder.parentElement) {
    aiPlaceholder.remove();
  }

  const timestamp = new Date().toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });

  // Create loading card in AI Copilot view
  const card = document.createElement("div");
  card.className = "ai-card loading";

  const cardHeader = document.createElement("div");
  cardHeader.className = "ai-card-header";

  const cardTitle = document.createElement("div");
  cardTitle.className = "ai-card-title";
  cardTitle.innerHTML = `<span class="ai-sparkle">✦</span> Gemini Copilot`;

  const cardTime = document.createElement("span");
  cardTime.className = "ai-card-time";
  cardTime.textContent = timestamp;

  cardHeader.appendChild(cardTitle);
  cardHeader.appendChild(cardTime);

  const cardQuestion = document.createElement("div");
  cardQuestion.className = "ai-card-question";
  cardQuestion.textContent = `"${questionText}"`;

  const cardAnswer = document.createElement("div");
  cardAnswer.className = "ai-card-answer";
  cardAnswer.innerHTML = `<em>Analyzing question & context...</em>`;

  card.appendChild(cardHeader);
  card.appendChild(cardQuestion);
  card.appendChild(cardAnswer);

  if (aiAnswersBox) {
    aiAnswersBox.appendChild(card);
    aiAnswersBox.scrollTop = aiAnswersBox.scrollHeight;
  }

  // Get last 5 transcript items for rolling context
  const recentContext = transcriptHistory
    .slice(-6)
    .map((item) => `[${item.timestamp}] ${item.text}`)
    .join("\n");

  // Call Gemini
  requestGeminiAnswer(questionText, recentContext)
    .then((answerHtml) => {
      card.classList.remove("loading");
      cardAnswer.innerHTML = answerHtml;
      if (aiAnswersBox) aiAnswersBox.scrollTop = aiAnswersBox.scrollHeight;
    })
    .catch((err) => {
      card.classList.remove("loading");
      cardAnswer.innerHTML = `<span style="color: #f87171;">Error: ${err.message || err}</span>`;
    });
}

function textSimilarity(a: string, b: string): number {
  if (a === b) return 1;
  const setA = new Set(a.toLowerCase().split(/\s+/));
  const setB = new Set(b.toLowerCase().split(/\s+/));
  const intersection = new Set([...setA].filter((x) => setB.has(x)));
  return (2.0 * intersection.size) / (setA.size + setB.size || 1);
}

// Clear AI Answers button
clearAiBtn?.addEventListener("click", () => {
  if (aiAnswersBox) {
    aiAnswersBox.innerHTML = `
      <div id="ai-placeholder" class="ai-placeholder">
        <span class="ai-empty-icon">✦</span>
        <p>Gemini AI is ready to detect and answer questions, quizzes, or tests from the live audio stream.</p>
      </div>
    `;
  }
});

// ==========================================
// Live Audio & Transcription Events
// ==========================================
interface AudioStatusPayload {
  status: string;
  message: string;
  is_active: boolean;
}

interface AudioLevelPayload {
  rms: number;
  peak: number;
}

interface ModelStatusPayload {
  status: "downloading" | "loading" | "ready" | "error";
  message: string;
}

interface TranscriptSegmentPayload {
  id: string;
  text: string;
  is_final: boolean;
  timestamp: string;
}

// Full transcript history for copying and context
const transcriptHistory: { timestamp: string; text: string }[] = [];

// Audio status
listen<AudioStatusPayload>("audio-status", (event) => {
  const { status, message, is_active } = event.payload;

  if (audioBadge && audioStatusLabel) {
    audioBadge.className = `audio-badge ${is_active ? "active" : status === "error" ? "error" : "inactive"}`;
    audioStatusLabel.textContent = is_active ? "System Audio Active" : status === "error" ? "Capture Error" : "Inactive";
  }

  if (statusDetail) {
    statusDetail.textContent = message;
  }
});

// Model loading status
listen<ModelStatusPayload>("model-status", (event) => {
  const { status, message } = event.payload;
  if (modelStatus) {
    modelStatus.textContent = message;
    if (status === "ready") {
      modelStatus.style.color = "rgba(52, 211, 153, 0.9)";
      modelStatus.style.borderColor = "rgba(16, 185, 129, 0.3)";
      modelStatus.style.background = "rgba(16, 185, 129, 0.1)";
    } else if (status === "error") {
      modelStatus.style.color = "rgba(248, 113, 113, 0.9)";
      modelStatus.style.borderColor = "rgba(239, 68, 68, 0.3)";
      modelStatus.style.background = "rgba(239, 68, 68, 0.1)";
    }
  }
});

// Live transcription event
listen<TranscriptSegmentPayload>("transcription-event", (event) => {
  const { text, timestamp } = event.payload;
  const cleanText = text.trim();

  if (!cleanText || !transcriptBox) return;

  // Remove placeholder on first transcript
  if (placeholder && placeholder.parentElement) {
    placeholder.remove();
  }

  const isScrolledToBottom =
    transcriptBox.scrollHeight - transcriptBox.scrollTop <= transcriptBox.clientHeight + 60;

  // Add to history
  transcriptHistory.push({ timestamp, text: cleanText });

  // Create entry element
  const entry = document.createElement("div");
  entry.className = "transcript-entry";

  const header = document.createElement("div");
  header.className = "entry-header";

  const timeEl = document.createElement("span");
  timeEl.className = "entry-time";
  timeEl.textContent = timestamp || new Date().toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });

  const askAiBtn = document.createElement("button");
  askAiBtn.className = "btn-ask-entry";
  askAiBtn.innerHTML = `✦ Ask AI`;
  askAiBtn.title = "Ask Gemini to answer or explain this snippet";
  askAiBtn.addEventListener("click", (e) => {
    e.stopPropagation();
    triggerAiQuestionAnswer(cleanText, true);
  });

  header.appendChild(timeEl);
  header.appendChild(askAiBtn);

  const textEl = document.createElement("div");
  textEl.className = "entry-text";
  textEl.textContent = cleanText;

  entry.appendChild(header);
  entry.appendChild(textEl);
  transcriptBox.appendChild(entry);

  if (isScrolledToBottom) {
    transcriptBox.scrollTop = transcriptBox.scrollHeight;
  }

  // Check if Auto-Answer is enabled and if this or recent text is a question
  if (autoAnswerCheckbox?.checked && isQuestionLike(cleanText)) {
    // Collect rolling window around the question
    const questionSnippet = cleanText;
    triggerAiQuestionAnswer(questionSnippet, false);
  }
});

// Copy transcript button
copyBtn?.addEventListener("click", async () => {
  if (transcriptHistory.length === 0) return;

  const fullText = transcriptHistory
    .map((item) => `[${item.timestamp}] ${item.text}`)
    .join("\n");

  try {
    await navigator.clipboard.writeText(fullText);
    if (copyBtn) {
      const originalText = copyBtn.querySelector("span")?.textContent || "Copy";
      copyBtn.classList.add("copied");
      const span = copyBtn.querySelector("span");
      if (span) span.textContent = "Copied!";
      setTimeout(() => {
        copyBtn.classList.remove("copied");
        if (span) span.textContent = originalText;
      }, 1500);
    }
  } catch (err) {
    console.error("Failed to copy transcript", err);
  }
});

// Clear transcript button
clearBtn?.addEventListener("click", () => {
  transcriptHistory.length = 0;
  if (transcriptBox) {
    transcriptBox.innerHTML = `
      <div id="placeholder" class="placeholder">
        Capturing system audio (YouTube, Zoom, meetings, media). Transcribing in real time...
      </div>
    `;
  }
});

// Real-time audio meter level
let decayTimer: number | null = null;

listen<AudioLevelPayload>("audio-level", (event) => {
  const { rms, peak } = event.payload;

  if (audioMeterBar) {
    const visualLevel = Math.min(100, Math.max(0, Math.round(Math.pow(rms, 0.7) * 200)));
    audioMeterBar.style.width = `${visualLevel}%`;

    if (decayTimer) {
      clearTimeout(decayTimer);
    }
    decayTimer = window.setTimeout(() => {
      if (audioMeterBar) {
        audioMeterBar.style.width = "0%";
      }
    }, 150);
  }

  if (statusDetail && rms > 0.005) {
    statusDetail.textContent = `Capturing audio (Peak: ${(peak * 100).toFixed(0)}%, RMS: ${(rms * 100).toFixed(1)}%)`;
  }
});