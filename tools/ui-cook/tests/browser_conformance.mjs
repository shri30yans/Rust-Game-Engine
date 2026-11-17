import { existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { pathToFileURL } from "node:url";

const [uiDir, uaPath, expectedPath] = process.argv.slice(2);
const ua = readFileSync(uaPath, "utf8");
const { cases } = JSON.parse(readFileSync(expectedPath, "utf8"));
const browserPath = process.env.CHROME_BIN ?? "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";

if (!existsSync(browserPath)) {
  console.log(`SKIPPED browser conformance: Chrome is not installed at ${browserPath}`);
  process.exit(0);
}

const profile = mkdtempSync(join(tmpdir(), "ui-conform-chrome-"));
const browser = spawn(browserPath, [
  "--headless=new",
  "--no-first-run",
  "--no-default-browser-check",
  "--disable-gpu",
  "--remote-debugging-port=0",
  `--user-data-dir=${profile}`,
  "about:blank",
], { stdio: "ignore" });

let socket;
try {
  const portFile = join(profile, "DevToolsActivePort");
  let port;
  for (let attempt = 0; attempt < 100; attempt += 1) {
    try {
      port = readFileSync(portFile, "utf8").split("\n")[0];
      break;
    } catch {
      await new Promise((done) => setTimeout(done, 50));
    }
  }
  if (!port) throw new Error("Chrome did not expose its DevTools port");
  const targets = await fetch(`http://127.0.0.1:${port}/json`).then((response) => response.json());
  const target = targets.find((item) => item.type === "page");
  if (!target) throw new Error("Chrome did not create a page target");
  socket = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((resolveOpen, reject) => {
    socket.addEventListener("open", resolveOpen, { once: true });
    socket.addEventListener("error", reject, { once: true });
  });
  let nextId = 0;
  const pending = new Map();
  socket.addEventListener("message", (event) => {
    const message = JSON.parse(event.data);
    if (message.id === undefined) return;
    const request = pending.get(message.id);
    if (!request) return;
    pending.delete(message.id);
    if (message.error) request.reject(new Error(message.error.message));
    else request.resolve(message.result);
  });
  const send = (method, params = {}) => new Promise((resolveResult, reject) => {
    const id = ++nextId;
    pending.set(id, { resolve: resolveResult, reject });
    socket.send(JSON.stringify({ id, method, params }));
  });

  await send("Page.enable");
  await send("Runtime.enable");
  for (const testCase of cases) {
    await send("Page.navigate", { url: pathToFileURL(resolve(testCase.html)).href });
    await new Promise((done) => setTimeout(done, 75));
    const setup = `(() => {
      const ua = ${JSON.stringify(ua)};
      const sample = ${JSON.stringify(testCase.sample)};
      const style = document.createElement('style');
      style.dataset.uiConformUa = 'true';
      style.textContent = ua;
      document.head.prepend(style);
      const segment = (node) => {
        const classes = [...node.classList].map((name) => '.' + name).join('');
        const id = node.id ? '#' + node.id : '';
        return node.tagName.toLowerCase() + classes + id;
      };
      const assignChildren = (node, parentPath) => {
        const counts = new Map();
        for (const child of node.children) {
          const base = segment(child);
          const index = counts.get(base) ?? 0;
          counts.set(base, index + 1);
          const path = parentPath + '>' + base + ':' + index;
          child.dataset.uiConformSource = path;
          assignChildren(child, path);
        }
      };
      document.body.dataset.uiConformSource = 'body';
      assignChildren(document.body, 'body');
      const assignRow = (node, path) => {
        node.dataset.uiConformSource = path;
        const counts = new Map();
        for (const child of node.children) {
          const base = segment(child);
          const index = counts.get(base) ?? 0;
          counts.set(base, index + 1);
          assignRow(child, path + '>' + base + ':' + index);
        }
      };
      for (const template of [...document.querySelectorAll('template[data-repeat]')]) {
        const source = template.dataset.uiConformSource;
        const rows = sample[template.dataset.repeat];
        if (!Array.isArray(rows)) throw new Error('sample list missing: ' + template.dataset.repeat);
        const root = template.content.firstElementChild;
        if (!root) throw new Error('repeat template has no root element');
        for (const [index, row] of rows.entries()) {
          const clone = root.cloneNode(true);
          clone.dataset.uiConformRow = template.dataset.repeat + ':' + index;
          assignRow(clone, source + '[' + index + ']/template');
          template.parentNode.insertBefore(clone, template);
        }
        template.remove();
      }
      const lookup = (name, element) => {
        if (name.startsWith('item.')) {
          const rowRoot = element.closest('[data-ui-conform-row]');
          if (!rowRoot) throw new Error('item binding outside sample row: ' + name);
          const [listName, rowIndexText] = rowRoot.dataset.uiConformRow.split(':');
          return sample[listName][Number(rowIndexText)][name.slice(5)];
        }
        return sample[name];
      };
      for (const element of [...document.querySelectorAll('[data-show-if]')]) {
        const expression = element.dataset.showIf;
        const negated = expression.startsWith('!');
        const value = lookup(negated ? expression.slice(1) : expression, element);
        if (typeof value !== 'boolean') throw new Error('sample flag missing or mistyped: ' + expression);
        if (value === negated) element.remove();
        else element.removeAttribute('data-show-if');
      }
      const format = (source, element) => source.replace(/\\{([^{}]+)\\}/g, (_match, slot) => {
        const separator = slot.lastIndexOf(':');
        const name = separator < 0 ? slot : slot.slice(0, separator);
        const value = lookup(name, element);
        if (value === undefined) throw new Error('sample value missing: ' + name);
        if (separator < 0) return String(value);
        const decimals = Number(slot.slice(separator + 1));
        return Number(value).toFixed(decimals);
      });
      const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
      const textNodes = [];
      while (walker.nextNode()) textNodes.push(walker.currentNode);
      for (const text of textNodes) text.nodeValue = format(text.nodeValue, text.parentElement);
      for (const element of [...document.querySelectorAll('[data-bind]')]) {
        const value = sample[element.dataset.bind];
        if (value === undefined) throw new Error('sample value missing: ' + element.dataset.bind);
        if (element instanceof HTMLInputElement) {
          if (element.type === 'checkbox') element.checked = Boolean(value);
          else element.value = String(value);
        } else if (element instanceof HTMLProgressElement) {
          element.value = Number(value);
        }
      }
      for (const element of [...document.querySelectorAll('[data-disabled]')]) {
        const value = lookup(element.dataset.disabled, element);
        if (typeof value !== 'boolean') throw new Error('sample disabled flag missing: ' + element.dataset.disabled);
        if (value && 'disabled' in element) element.disabled = true;
      }
      for (const element of document.querySelectorAll('[data-ui-conform-row]')) {
        delete element.dataset.uiConformRow;
      }
      return [...document.querySelectorAll('[data-ui-conform-source]')].map((node) => {
        const rect = node.getBoundingClientRect();
        return [node.dataset.uiConformSource, { x: rect.x, y: rect.y, w: rect.width, h: rect.height }];
      });
    })()`;
    const setupResult = await send("Runtime.evaluate", { expression: setup, returnByValue: true });
    if (setupResult.exceptionDetails) throw new Error(`${testCase.screen}: ${setupResult.exceptionDetails.text}`);

    for (const size of testCase.sizes) {
      const shortSide = Math.min(size.physicalWidth, size.physicalHeight);
      const deviceScaleFactor = shortSide / 720;
      const width = Math.round(size.physicalWidth / deviceScaleFactor);
      const height = Math.round(size.physicalHeight / deviceScaleFactor);
      await send("Emulation.setDeviceMetricsOverride", {
        width,
        height,
        deviceScaleFactor,
        mobile: true,
        screenWidth: width,
        screenHeight: height,
      });
      const response = await send("Runtime.evaluate", {
        expression: `(() => [...document.querySelectorAll('[data-ui-conform-source]')].map(node => {
          const rect = node.getBoundingClientRect();
          return [node.dataset.uiConformSource, { x: rect.x, y: rect.y, w: rect.width, h: rect.height }];
        }))()`,
        returnByValue: true,
      });
      if (response.exceptionDetails) throw new Error(`${testCase.screen}: ${response.exceptionDetails.text}`);
      const actual = new Map(response.result.value);
      const expected = size.rects;
      const tolerance = 1;
      const failures = [];
      for (const [source, box] of Object.entries(expected)) {
        const browserBox = actual.get(source);
        if (!browserBox) {
          failures.push(`${source}: missing from browser`);
          continue;
        }
        for (const axis of box.text ? ['x', 'y', 'h'] : ['x', 'y', 'w', 'h']) {
          const chrome = browserBox[axis] * deviceScaleFactor;
          if (Math.abs(box[axis] - chrome) > tolerance) {
            failures.push(`${source} ${axis}: engine=${box[axis]} Chrome=${chrome}`);
          }
        }
      }
      if (failures.length) {
        throw new Error(`${testCase.screen} at ${size.physicalWidth}x${size.physicalHeight}:\n${failures.join('\n')}`);
      }
      console.log(`Chrome conformance passed for ${testCase.screen} at ${size.physicalWidth}x${size.physicalHeight} (DSF ${deviceScaleFactor.toFixed(4)})`);
    }
  }
} finally {
  if (socket) socket.close();
  browser.kill("SIGTERM");
  await Promise.race([once(browser, "close"), new Promise((done) => setTimeout(done, 2000))]);
  rmSync(profile, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
}
