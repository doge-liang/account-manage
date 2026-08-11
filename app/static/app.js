/* 账号管家 — 前端逻辑（无依赖，纯原生 JS） */
"use strict";

/* ==================== 类别与字段定义 ==================== */
const CATEGORIES = {
  ai_member: {
    label: "AI 会员", icon: "robot", color: "#4f46e5",
    fields: [
      { k: "plan", label: "套餐" },
      { k: "monthly_fee", label: "月费", type: "number", ph: "如 200" },
      { k: "currency", label: "币种", type: "select", options: ["USD", "CNY", "其他"] },
      { k: "quota_type", label: "额度类型", type: "select", options: [
        { v: "subscription", l: "订阅（按周期）" },
        { v: "quota", l: "用量额度（如 5h/周）" },
        { v: "payg", l: "按量计费（余额）" }] },
      { k: "billing_date", label: "账单 / 扣费日", type: "date", ph: "每月扣费日" },
      { k: "reset_date", label: "用量重置日", type: "date", ph: "额度重置日期" },
      { k: "balance", label: "余额（按量）", type: "number", ph: "按量计费时填" },
      { k: "remaining_usage", label: "剩余用量", ph: "如 2.1/15h 或 40%" },
      { k: "last_checked", label: "最近核对日", type: "date" },
      { k: "key_alias", label: "Key 别名" },
      { k: "key_last4", label: "Key 末四位" },
      { k: "kp_ref", label: "KeePass 存放处", ph: "KeePassXC: AI Keys/<厂商>/<别名>" },
    ],
    keyFields: ["monthly_fee", "currency", "quota_type", "billing_date", "reset_date", "remaining_usage", "balance"],
  },
  api: {
    label: "按量 API", icon: "api", color: "#0891b2",
    fields: [
      { k: "balance", label: "余额", type: "number" },
      { k: "currency", label: "币种", type: "select", options: ["USD", "CNY", "其他"] },
      { k: "billing_date", label: "账单日", type: "date" },
      { k: "last_checked", label: "最近核对日", type: "date" },
      { k: "key_alias", label: "Key 别名" },
      { k: "key_last4", label: "Key 末四位" },
      { k: "kp_ref", label: "KeePass 存放处", ph: "KeePassXC: AI Keys/<厂商>/<别名>" },
    ],
    keyFields: ["balance", "currency", "billing_date"],
  },
  email: {
    label: "邮箱", icon: "email", color: "#d97706",
    fields: [
      { k: "provider", label: "服务商", type: "select", options: ["Gmail", "Outlook", "QQ 邮箱", "163 网易", "其他"] },
      { k: "aliases", label: "别名 / 其他地址" },
      { k: "recovery", label: "备用邮箱" },
    ],
    keyFields: ["provider"],
  },
  phone: {
    label: "手机号", icon: "cellphone", color: "#16a34a",
    fields: [
      { k: "country", label: "地区", type: "select", options: ["境内", "境外"] },
      { k: "carrier", label: "运营商", ph: "如 中国移动 / T-Mobile" },
      { k: "billing_date", label: "账单日", ph: "如 每月 5 日" },
      { k: "balance", label: "话费余额", type: "number" },
      { k: "query_url", label: "话费查询链接", ph: "可配置，一键跳转手动查" },
    ],
    keyFields: ["carrier", "country", "billing_date", "balance"],
  },
  wechat: { label: "微信", icon: "wechat", color: "#16a34a", fields: [{ k: "region", label: "地区", type: "select", options: ["境内", "境外"] }], keyFields: [] },
  public_account: { label: "公众号", icon: "bullhorn", color: "#dc2626", fields: [], keyFields: [] },
  qq: { label: "QQ", icon: "penguin", color: "#2563eb", fields: [], keyFields: [] },
  zlibrary: { label: "Z-library", icon: "book-open", color: "#7c3aed", fields: [{ k: "member_until", label: "会员到期日", type: "date" }], keyFields: ["member_until"] },
  apple: { label: "Apple ID", icon: "apple", color: "#4b5563", fields: [{ k: "region", label: "地区", type: "select", options: ["境内", "境外"] }], keyFields: ["region"] },
  other: { label: "其他", icon: "package", color: "#64748b", fields: [], keyFields: [] },
};
const CATEGORY_KEYS = Object.keys(CATEGORIES);

/* 图标渲染：icons.svg 符号引用（离线可用，无 emoji） */
const IC = (name) => `<svg class="ic" aria-hidden="true"><use href="icons.svg#mdi-${name}"></use></svg>`;

const RELATION_TYPES = ["登录邮箱", "注册邮箱", "备用邮箱", "绑定手机", "同一主体", "支付方式", "其他"];

const STATUS_LABEL = { active: "active", inactive: "inactive", expired: "expired" };

/* ==================== 状态 ==================== */
let state = {
  data: null,           // {accounts, relations, query_links, settings}
  tab: "dashboard",
  search: "",
  catFilter: "",
  statusFilter: "",
  editingId: null,      // 详情抽屉打开的账号
  usageConfigs: [],     // 用量接口配置（含 cache）
  usageProviders: [],   // 内置 provider 列表
};
const $ = (sel) => document.querySelector(sel);

/* ==================== 工具函数 ==================== */
function esc(s) {
  return String(s ?? "").replace(/[&<>"']/g, (c) => (
    { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
}
function catInfo(cat) { return CATEGORIES[cat] || CATEGORIES.other; }
function findAccount(id) { return (state.data.accounts || []).find((a) => a.id === id); }
function fmtMoney(v, cur) {
  if (v === null || v === undefined || v === "") return "—";
  const n = Number(v);
  if (isNaN(n)) return String(v);
  return `${n.toLocaleString("zh-CN")} ${cur || ""}`.trim();
}
function daysUntil(dateStr) {
  if (!dateStr) return null;
  const today = new Date(); today.setHours(0, 0, 0, 0);
  const d = new Date(dateStr + "T00:00:00");
  if (isNaN(d)) return null;
  return Math.round((d - today) / 86400000);
}
function dateLabel(dateStr) {
  if (!dateStr) return "—";
  const d = daysUntil(dateStr);
  if (d === null) return esc(dateStr);
  if (d < 0) return `${esc(dateStr)}（已过 ${-d} 天）`;
  if (d === 0) return `${esc(dateStr)}（今天）`;
  return `${esc(dateStr)}（还有 ${d} 天）`;
}
function relCountdown(acc) {
  const f = acc.fields || {};
  if (acc.category !== "ai_member") return null;
  const d = f.reset_date || f.billing_date;
  return d ? daysUntil(d) : null;
}

/* ==================== API ==================== */
async function api(path, opts = {}) {
  const res = await fetch(path, {
    headers: { "Content-Type": "application/json" },
    ...opts,
  });
  let body = null;
  try { body = await res.json(); } catch { /* ignore */ }
  if (!res.ok) throw new Error((body && body.error) || `HTTP ${res.status}`);
  return body;
}
const apiGet = (p) => api(p);
const apiPost = (p, obj) => api(p, { method: "POST", body: JSON.stringify(obj) });
const apiPut = (p, obj) => api(p, { method: "PUT", body: JSON.stringify(obj) });
const apiDel = (p) => api(p, { method: "DELETE" });

async function reload() {
  const data = await apiGet("/api/data");
  state.data = data;
  $("#app-name").textContent = (data.settings && data.settings.name) || "账号管家";
  try {
    const u = await apiGet("/api/usage");
    state.usageConfigs = (u && u.configs) || [];
    const p = await apiGet("/api/usage/providers");
    state.usageProviders = (p && p.providers) || [];
  } catch { state.usageConfigs = []; state.usageProviders = []; }
  render();
}

/* ==================== 导航 ==================== */
function switchTab(tab) {
  state.tab = tab;
  document.querySelectorAll(".tab").forEach((t) => t.classList.toggle("active", t.dataset.tab === tab));
  document.querySelectorAll(".tab-panel").forEach((p) => p.classList.toggle("active", p.id === `tab-${tab}`));
  render();
}

/* ==================== 渲染入口 ==================== */
function render() {
  if (!state.data) return;
  if (state.tab === "dashboard") renderDashboard();
  else if (state.tab === "accounts") renderAccounts();
  else if (state.tab === "vault") renderVault();
  else if (state.tab === "settings") renderSettings();
}

/* ==================== 仪表盘 ==================== */
function renderDashboard() {
  renderDashboardUsage();
  const accs = state.data.accounts;
  const active = accs.filter((a) => a.status !== "expired");
  const ai = accs.filter((a) => a.category === "ai_member");
  const usd = ai.reduce((s, a) => s + (Number(a.fields?.monthly_fee) || 0) * (a.fields?.currency === "USD" ? 1 : 0), 0);
  const cny = ai.reduce((s, a) => s + (Number(a.fields?.monthly_fee) || 0) * (a.fields?.currency === "CNY" ? 1 : 0), 0);

  const statCards = [
    { num: accs.length, label: "账号总数", color: "var(--text)" },
    { num: ai.length, label: "AI 会员数", color: "#4f46e5" },
    { num: `$${usd}`, label: "AI 月费 (USD)", color: "#16a34a" },
    { num: `¥${cny}`, label: "AI 月费 (CNY)", color: "#d97706" },
    { num: accs.filter((a) => a.category === "phone").length, label: "手机号", color: "#0891b2" },
    { num: accs.filter((a) => a.category === "email").length, label: "邮箱", color: "#2563eb" },
  ];
  $("#stat-grid").innerHTML = statCards.map((c) => `
    <div class="stat-card"><div class="stat-num" style="color:${c.color}">${esc(c.num)}</div>
    <div class="stat-label">${esc(c.label)}</div></div>`).join("");

  // 即将发生：AI 会员扣费/重置、手机账单日 在 7 天内
  const upcoming = [];
  for (const a of active) {
    const f = a.fields || {};
    const checks = [];
    if (a.category === "ai_member" || a.category === "api") {
      if (f.billing_date) checks.push(["账单日", f.billing_date]);
      if (f.reset_date) checks.push(["用量重置日", f.reset_date]);
    }
    if (a.category === "phone" && f.billing_date) {
      checks.push(["手机账单日", f.billing_date]);
    }
    if (a.category === "zlibrary" && f.member_until) checks.push(["会员到期", f.member_until]);
    for (const [label, date] of checks) {
      const d = daysUntil(date);
      if (d !== null && d >= 0 && d <= 7) {
        upcoming.push({ acc: a, label, date, d });
      }
    }
  }
  upcoming.sort((x, y) => x.d - y.d);
  $("#dash-upcoming").innerHTML = upcoming.length
    ? upcoming.map((u) => `
      <div class="list-item">
        <span>${IC(catInfo(u.acc.category).icon)}</span>
        <div class="li-main">
          <div class="li-title">${esc(u.acc.name)}</div>
          <div class="li-sub">${esc(u.label)}</div>
        </div>
        <div class="li-right ${u.d <= 2 ? "badge-warn" : ""}" style="border-radius:999px;padding:2px 9px">${u.d === 0 ? "今天" : u.d + " 天后"}</div>
      </div>`).join("")
    : `<div class="empty-hint">未来 7 天没有到期事项 ${IC("party-popper")}</div>`;

  // 余额/用量预警：用量类账号缺余额/剩余用量，或重置日已过
  const alerts = [];
  for (const a of active) {
    const f = a.fields || {};
    if (a.category === "ai_member" || a.category === "api") {
      const needBalance = a.category === "api" || f.quota_type === "payg";
      if (needBalance && (f.balance === "" || f.balance === undefined || f.balance === null)) {
        alerts.push({ acc: a, text: "按量/余额未填写" });
      }
      if ((a.category === "ai_member" && f.quota_type === "quota")
          && (f.remaining_usage === "" || f.remaining_usage === undefined || f.remaining_usage === null)) {
        alerts.push({ acc: a, text: "剩余用量未填写" });
      }
      if (f.reset_date) {
        const d = daysUntil(f.reset_date);
        if (d !== null && d < 0) alerts.push({ acc: a, text: `重置日已过 ${-d} 天` });
      }
    }
    if (a.category === "phone" && (f.balance === "" || f.balance === undefined || f.balance === null)) {
      alerts.push({ acc: a, text: "话费余额未填写" });
    }
  }
  $("#dash-alerts").innerHTML = alerts.length
    ? alerts.map((al) => `
      <div class="list-item">
        <span>${IC("alert")}</span>
        <div class="li-main"><div class="li-title">${esc(al.acc.name)}</div>
        <div class="li-sub">${esc(al.text)}</div></div>
      </div>`).join("")
    : `<div class="empty-hint">没有余额/用量预警 ${IC("check-circle")}</div>`;

  // 月费汇总
  const byVendor = {};
  for (const a of ai) {
    const v = a.vendor || "未知";
    byVendor[v] = byVendor[v] || { usd: 0, cny: 0, n: 0 };
    const fee = Number(a.fields?.monthly_fee) || 0;
    if (a.fields?.currency === "USD") byVendor[v].usd += fee;
    else if (a.fields?.currency === "CNY") byVendor[v].cny += fee;
    byVendor[v].n += 1;
  }
  const rows = Object.entries(byVendor).map(([v, d]) => `
    <tr><td>${esc(v)}</td><td>${d.n} 个</td><td>$${d.usd}</td><td>¥${d.cny}</td></tr>`).join("");
  $("#dash-vendor-cost").innerHTML = `
    <table class="acc-table"><thead><tr><th>厂商</th><th>账号数</th><th>月费 USD</th><th>月费 CNY</th></tr></thead>
    <tbody>${rows || '<tr><td colspan="4" class="empty-hint">暂无 AI 会员</td></tr>'}</tbody></table>`;
}

/* ==================== 仪表盘用量进度条 ==================== */
function usageForAccount(accId) {
  // 返回 {used, total, percent, unit, fetched_at} 或 null
  const cfg = (state.usageConfigs || []).find((c) => c.account_id === accId && c.enabled);
  if (cfg && cfg.cache) return cfg.cache;
  return null;
}

function parseManualUsage(acc) {
  // 从 remaining_usage 手动填写的值里尽力解析 used/total，比如 "2.1/15h" / "40%"
  const f = acc.fields || {};
  const raw = String(f.remaining_usage || "").trim();
  if (!raw) return null;
  const m = raw.match(/(\d+(?:\.\d+)?)\s*\/\s*(\d+(?:\.\d+)?)/);
  if (m) {
    const used = parseFloat(m[1]), total = parseFloat(m[2]);
    return total > 0 ? { used, total, percent: Math.round(used / total * 1000) / 10, manual: true } : null;
  }
  const pct = raw.match(/(\d+(?:\.\d+)?)\s*%/);
  if (pct) return { used: null, total: null, percent: parseFloat(pct[1]), manual: true };
  return null;
}

function renderDashboardUsage() {
  const ai = (state.data.accounts || []).filter((a) => a.category === "ai_member" && a.status !== "expired");
  if (!ai.length) {
    $("#dash-usage").innerHTML = '<div class="empty-hint">暂无 AI 会员账号</div>';
    return;
  }
  const items = ai.map((a) => {
    const live = usageForAccount(a.id);
    const manual = parseManualUsage(a);
    return { acc: a, live, manual };
  }).filter((x) => x.live || x.manual);

  if (!items.length) {
    $("#dash-usage").innerHTML = '<div class="empty-hint">暂无用量数据。请在「设置 → 用量接口配置」添加接口，或在账号字段里填写剩余用量。</div>';
    return;
  }

  $("#dash-usage").innerHTML = items.map(({ acc, live, manual }) => {
    const data = live || manual;
    const pct = data.percent_used ?? null;
    const used = data.used;
    const total = data.total;
    const unit = (live && live.unit) || (acc.fields && acc.fields.quota_type === "quota" ? "" : "");
    let pctText = "—", fillCls = "", widthPct = 0;
    if (pct !== null) {
      pctText = `${pct}%`;
      widthPct = Math.max(0, Math.min(100, pct));
      if (pct >= 90) fillCls = "danger";
      else if (pct >= 70) fillCls = "warn";
    }
    const usedText = (used !== null && used !== undefined) ? fmtNum(used) : "—";
    const totalText = (total !== null && total !== undefined) ? fmtNum(total) : "—";
    const sem = data.percent_semantics === "remaining" ? "剩余" : "已用";
    let barText;
    if (used !== null && total !== null) {
      barText = `${usedText} / ${totalText}${unit ? " " + esc(unit) : ""}`;
    } else if (pct !== null) {
      barText = `${sem} ${pct}%`;
    } else {
      barText = "未配置";
    }
    const sourceLabel = live
      ? (live.fetched_at ? `自动 · ${fmtTimeAgo(live.fetched_at)}` : "自动")
      : "手动填写";
    return `
      <div class="usage-item">
        <div class="ui-head">
          <div class="ui-name">${IC("robot")}<span title="${esc(acc.name)}">${esc(acc.name)}</span></div>
          <span class="ui-pct ${fillCls ? "badge-warn" : ""}" style="${fillCls === 'danger' ? 'background:var(--danger-soft);color:var(--danger)' : fillCls === 'warn' ? 'background:var(--warn-soft);color:var(--warn)' : 'background:var(--primary-soft);color:var(--primary)'}">${esc(pctText)}</span>
        </div>
        <div class="usage-bar"><div class="ub-fill ${fillCls}" style="width:${widthPct}%"></div></div>
        <div class="ui-meta">
          <span>${esc(barText)}</span>
          <span>${esc(sourceLabel)}</span>
        </div>
      </div>`;
  }).join("");
}

function fmtNum(n) {
  if (n === null || n === undefined) return "—";
  const x = Number(n);
  if (isNaN(x)) return String(n);
  return Number.isInteger(x) ? String(x) : x.toFixed(2).replace(/\.?0+$/, "");
}

function fmtTimeAgo(iso) {
  if (!iso) return "—";
  const t = new Date(iso);
  if (isNaN(t)) return esc(iso);
  const diff = Math.max(0, (Date.now() - t.getTime()) / 1000);
  if (diff < 60) return "刚刚";
  if (diff < 3600) return `${Math.floor(diff / 60)} 分钟前`;
  if (diff < 86400) return `${Math.floor(diff / 3600)} 小时前`;
  return `${Math.floor(diff / 86400)} 天前`;
}

/* ==================== 账号列表 ==================== */
function renderAccounts() {
  // 类别筛选 chips
  const chips = [{ k: "", l: "全部" }].concat(CATEGORY_KEYS.map((k) => ({ k, l: catInfo(k).label })));
  $("#category-chips").innerHTML = chips.map((c) =>
    `<button class="chip ${state.catFilter === c.k ? "active" : ""}" data-cat="${c.k}">${c.k ? IC(catInfo(c.k).icon) : ""}${esc(c.l)}</button>`).join("");

  const q = state.search.trim().toLowerCase();
  let list = state.data.accounts.filter((a) => {
    if (state.catFilter && a.category !== state.catFilter) return false;
    if (state.statusFilter && a.status !== state.statusFilter) return false;
    if (!q) return true;
    const hay = [a.name, a.vendor, a.username, a.notes, JSON.stringify(a.fields || {})].join(" ").toLowerCase();
    return hay.includes(q);
  });
  list = [...list].sort((a, b) => a.name.localeCompare(b.name, "zh"));

  const tbody = $("#acc-tbody");
  tbody.innerHTML = list.map((a) => {
    const f = a.fields || {};
    let keyInfo = "";
    if (a.category === "ai_member") {
      const parts = [];
      if (f.quota_type === "payg" && f.balance !== "" && f.balance !== undefined) parts.push(`余额 ${fmtMoney(f.balance, f.currency)}`);
      else if (f.quota_type === "quota" && f.remaining_usage) parts.push(`剩余 ${esc(f.remaining_usage)}`);
      if (f.monthly_fee) parts.push(`${fmtMoney(f.monthly_fee, f.currency)}/月`);
      if (f.reset_date) parts.push(`重置 ${esc(f.reset_date)}`);
      keyInfo = parts.join(" · ") || "—";
    } else if (a.category === "api") {
      keyInfo = f.balance !== "" && f.balance !== undefined ? `余额 ${fmtMoney(f.balance, f.currency)}` : "—";
    } else if (a.category === "phone") {
      keyInfo = [f.carrier, f.country, f.balance ? `余额 ${f.balance}` : ""].filter(Boolean).join(" · ") || "—";
    } else if (a.category === "email") {
      keyInfo = f.provider || a.username || "—";
    } else {
      keyInfo = a.username || "—";
    }
    return `
    <tr data-id="${esc(a.id)}">
      <td><b>${esc(a.name)}</b>${a.username ? `<div class="li-sub" style="font-size:12px;color:var(--text-2)">${esc(a.username)}</div>` : ""}</td>
      <td>${esc(a.vendor || "—")}</td>
      <td><span class="badge badge-cat">${IC(catInfo(a.category).icon)} ${catInfo(a.category).label}</span></td>
      <td style="font-size:13px">${keyInfo}</td>
      <td><span class="badge ${esc(a.status)}">${STATUS_LABEL[a.status] || esc(a.status)}</span></td>
      <td style="white-space:nowrap">
        <button class="btn btn-sm" data-act="edit" data-id="${esc(a.id)}">编辑</button>
        <button class="btn btn-sm btn-danger" data-act="del" data-id="${esc(a.id)}">删除</button>
      </td>
    </tr>`;
  }).join("");
  $("#acc-empty").hidden = list.length > 0;

  // 绑定
  document.querySelectorAll("#category-chips .chip").forEach((c) =>
    c.onclick = () => { state.catFilter = c.dataset.cat; renderAccounts(); });
  document.querySelectorAll("#acc-tbody tr[data-id]").forEach((tr) => {
    tr.onclick = (e) => {
      if (e.target.closest("[data-act]")) return; // 按钮处理
      openDrawer(tr.dataset.id);
    };
  });
  document.querySelectorAll("#acc-tbody [data-act='edit']").forEach((b) =>
    b.onclick = (e) => { e.stopPropagation(); openForm(b.dataset.id); });
  document.querySelectorAll("#acc-tbody [data-act='del']").forEach((b) =>
    b.onclick = async (e) => {
      e.stopPropagation();
      const a = findAccount(b.dataset.id);
      if (!confirm(`确定删除「${a.name}」？\n关联它的关系也会一并删除。`)) return;
      const r = await apiDel(`/api/accounts/${b.dataset.id}`);
      await reload();
      flash(`已删除，顺带清理 ${r.removed_relations || 0} 条关联`);
    });
}

/* ==================== 详情抽屉 ==================== */
function openDrawer(id) {
  state.editingId = id;
  const a = findAccount(id);
  if (!a) return;
  const f = a.fields || {};
  $("#drawer-title").innerHTML = `${IC(catInfo(a.category).icon)} ${esc(a.name)}`;
  $("#drawer-sub").innerHTML = `${catInfo(a.category).label} · ${esc(a.vendor || "未知厂商")} · <span class="badge ${esc(a.status)}">${STATUS_LABEL[a.status] || esc(a.status)}</span>`;

  // 字段表
  const allFields = [{ k: "username", l: "登录账号 / 号码", v: a.username },
                     { k: "url", l: "控制台 / 官网", v: a.url },
                     { k: "notes", l: "备注", v: a.notes }]
    .concat(catInfo(a.category).fields.map((fd) => ({ k: fd.k, l: fd.label, v: f[fd.k] })))
    .filter((x) => x.v !== "" && x.v !== undefined && x.v !== null);
  const kvHtml = allFields.map((x) => `
    <div class="kv-item ${x.l === "备注" ? "full" : ""}">
      <div class="k">${esc(x.l)}</div>
      <div class="v">${x.k === "url" && x.v ? `<a href="${esc(x.v)}" target="_blank" rel="noopener">${esc(x.v)}</a>` : esc(x.v)}</div>
    </div>`).join("") || '<div class="empty-hint">暂无字段</div>';

  // 查询/跳转按钮
  const links = [];
  if (a.url) links.push({ label: "控制台", url: a.url });
  if (f.query_url) links.push({ label: "话费查询", url: f.query_url });
  for (const q of (state.data.query_links || [])) {
    if ((!q.category || q.category === a.category) && (!q.vendor || q.vendor === a.vendor)) {
      links.push({ label: q.label, url: q.url });
    }
  }
  const linksHtml = links.length
    ? `<div class="btn-row">${links.map((l) => `<a class="btn" href="${esc(l.url)}" target="_blank" rel="noopener">${IC("link-variant")} ${esc(l.label)}</a>`).join("")}</div>`
    : "";

  // 关联
  const rels = (state.data.relations || []).filter((r) => r.from === id || r.to === id);
  const relHtml = rels.map((r) => {
    const isFrom = r.from === id;
    const other = findAccount(isFrom ? r.to : r.from);
    if (!other) return "";
    return `
      <div class="list-item">
        <span>${IC(isFrom ? "arrow-right" : "arrow-left")}</span>
        <div class="li-main">
          <div class="li-title">${IC(catInfo(other.category).icon)} ${esc(other.name)}</div>
          <div class="li-sub">${esc(r.type)}${isFrom ? "" : `（反向）`}${r.note ? ` · ${esc(r.note)}` : ""}</div>
        </div>
        <button class="btn btn-sm" data-rel-del="${esc(r.id)}">解除</button>
      </div>`;
  }).join("") || '<div class="empty-hint">还没有关联账号</div>';

  $("#drawer-body").innerHTML = `
    ${linksHtml}
    <h3 style="margin:18px 0 8px">${IC("clipboard-text")} 字段</h3>
    <div class="kv-grid">${kvHtml}</div>
    <h3 style="margin:18px 0 8px">${IC("link-variant")} 关联账号（${rels.length}）</h3>
    <div class="list-block" id="rel-list">${relHtml}</div>
    <div class="btn-row">
      <button class="btn" id="btn-add-rel">+ 添加关联</button>
      <button class="btn btn-sm" id="btn-edit-acc">编辑账号</button>
      <button class="btn btn-sm btn-danger" id="btn-del-acc">删除账号</button>
    </div>`;

  $("#drawer-backdrop").hidden = false;
  $("#drawer").hidden = false;

  $("#drawer-close").onclick = closeDrawer;
  $("#drawer-backdrop").onclick = closeDrawer;
  document.querySelectorAll("[data-rel-del]").forEach((b) => b.onclick = async () => {
    if (!confirm("解除这条关联？")) return;
    await apiDel(`/api/relations/${b.dataset.relDel}`);
    await reload();
    openDrawer(id);
  });
  $("#btn-add-rel").onclick = () => openRelationForm(id);
  $("#btn-edit-acc").onclick = () => openForm(id);
  $("#btn-del-acc").onclick = async () => {
    if (!confirm(`确定删除「${a.name}」？`)) return;
    const r = await apiDel(`/api/accounts/${id}`);
    closeDrawer();
    await reload();
    flash(`已删除，顺带清理 ${r.removed_relations || 0} 条关联`);
  };
}
function closeDrawer() {
  state.editingId = null;
  $("#drawer-backdrop").hidden = true;
  $("#drawer").hidden = true;
}

/* ==================== 账号表单 ==================== */
function openForm(id) {
  const editing = id ? findAccount(id) : null;
  $("#modal-title").textContent = editing ? "编辑账号" : "新增账号";
  const cat = editing ? editing.category : "ai_member";
  const f = editing ? (editing.fields || {}) : {};

  const commonHtml = `
    <label>显示名称 *<input class="input-box" id="f-name" value="${esc(editing ? editing.name : "")}" placeholder="如 ChatGPT Max 20x"></label>
    <label>类别
      <select id="f-category">${CATEGORY_KEYS.map((k) => `<option value="${k}" ${k === cat ? "selected" : ""}>${catInfo(k).label}</option>`).join("")}</select>
    </label>
    <label>厂商 / 运营商<input class="input-box" id="f-vendor" value="${esc(editing ? editing.vendor : "")}" placeholder="如 OpenAI / 中国移动"></label>
    <label>登录账号 / 号码<input class="input-box" id="f-username" value="${esc(editing ? editing.username : "")}" placeholder="邮箱、手机号或用户名"></label>
    <label class="full">控制台 / 官网 URL<input class="input-box" id="f-url" value="${esc(editing ? editing.url : "")}" placeholder="https://…"></label>
    <label>状态
      <select id="f-status">${["active", "inactive", "expired"].map((s) => `<option value="${s}" ${(editing ? editing.status : "active") === s ? "selected" : ""}>${s}</option>`).join("")}</select>
    </label>`;

  $("#modal-body").innerHTML = `
    <div class="form-grid">
      ${commonHtml}
      <div class="form-section" id="form-section-label">${IC("clipboard-text")} 类别字段</div>
      <div id="category-fields"></div>
      <label class="full">备注<textarea id="f-notes" placeholder="自由备注">${esc(editing ? editing.notes : "")}</textarea></label>
    </div>`;

  $("#modal-backdrop").hidden = false;
  $("#f-category").onchange = () => renderCategoryFields($("#f-category").value);
  renderCategoryFields(cat, f);

  $("#modal-close").onclick = closeForm;
  $("#modal-cancel").onclick = closeForm;
  $("#modal-backdrop").onclick = (e) => { if (e.target === $("#modal-backdrop")) closeForm(); };
  $("#modal-save").onclick = () => saveForm(editing ? editing.id : null);
}

function renderCategoryFields(cat, existing = {}) {
  const fields = catInfo(cat).fields;
  const html = fields.map((fd) => {
    const val = existing[fd.k] ?? "";
    let control = "";
    if (fd.type === "select") {
      const opts = (fd.options || []).map((o) => {
        const v = typeof o === "object" ? o.v : o;
        const l = typeof o === "object" ? o.l : o;
        return `<option value="${esc(v)}" ${String(val) === String(v) ? "selected" : ""}>${esc(l)}</option>`;
      }).join("");
      control = `<select class="input-box" data-fk="${fd.k}">${opts}</select>`;
    } else if (fd.type === "date") {
      control = `<input class="input-box" type="date" data-fk="${fd.k}" value="${esc(val)}">`;
    } else if (fd.type === "number") {
      control = `<input class="input-box" type="number" step="any" data-fk="${fd.k}" value="${esc(val)}" placeholder="${esc(fd.ph || "")}">`;
    } else {
      control = `<input class="input-box" data-fk="${fd.k}" value="${esc(val)}" placeholder="${esc(fd.ph || "")}">`;
    }
    return `<label>${esc(fd.label)}${control}</label>`;
  }).join("");
  $("#category-fields").outerHTML = `<div id="category-fields" class="form-grid" style="grid-column:1/-1">${html}</div>`;
}

function closeForm() {
  $("#modal-backdrop").hidden = true;
}

async function saveForm(id) {
  const name = $("#f-name").value.trim();
  if (!name) { alert("显示名称不能为空"); return; }
  const fields = {};
  document.querySelectorAll("#category-fields [data-fk]").forEach((el) => {
    fields[el.dataset.fk] = el.value.trim();
  });
  const payload = {
    category: $("#f-category").value,
    name,
    vendor: $("#f-vendor").value.trim(),
    username: $("#f-username").value.trim(),
    url: $("#f-url").value.trim(),
    status: $("#f-status").value,
    notes: $("#f-notes").value.trim(),
    fields,
  };
  try {
    if (id) {
      await apiPut(`/api/accounts/${id}`, payload);
    } else {
      await apiPost("/api/accounts", payload);
    }
  } catch (e) { alert(`保存失败：${e.message}`); return; }
  closeForm();
  await reload();
  if (id) openDrawer(id);
  flash(id ? "已更新" : "已新增");
}

/* ==================== 关联表单 ==================== */
function openRelationForm(accId) {
  const others = state.data.accounts.filter((a) => a.id !== accId);
  $("#modal-title").textContent = "添加关联";
  $("#modal-body").innerHTML = `
    <div class="form-grid">
      <label>当前账号
        <input class="input-box" value="${esc(findAccount(accId).name)}" disabled>
      </label>
      <label>关联到
        <select id="rel-target">${others.map((a) => `<option value="${esc(a.id)}">${esc(a.name)}</option>`).join("")}</select>
      </label>
      <label>关系类型
        <select id="rel-type">${RELATION_TYPES.map((t) => `<option>${esc(t)}</option>`).join("")}</select>
      </label>
      <label class="full">备注<input class="input-box" id="rel-note" placeholder="如：AI 会员都用这个 Gmail 登录"></label>
    </div>`;
  $("#modal-backdrop").hidden = false;
  $("#modal-close").onclick = closeForm;
  $("#modal-cancel").onclick = closeForm;
  $("#modal-save").onclick = async () => {
    const payload = {
      from: accId,
      to: $("#rel-target").value,
      type: $("#rel-type").value,
      note: $("#rel-note").value.trim(),
    };
    try {
      await apiPost("/api/relations", payload);
    } catch (e) { alert(`添加失败：${e.message}`); return; }
    closeForm();
    await reload();
    openDrawer(accId);
  };
}

/* ==================== 密钥库 ==================== */
async function renderVault() {
  const info = await apiGet("/api/vault/info");
  $("#vault-info").innerHTML = info.exists ? `
    <div class="kv-item full"><div class="k">路径</div><div class="v">${esc(info.path)}</div></div>
    <div class="kv-item"><div class="k">大小</div><div class="v">${(info.size / 1024).toFixed(1)} KB</div></div>
    <div class="kv-item"><div class="k">最后修改</div><div class="v">${esc(info.mtime || "—")}</div></div>
    <div class="kv-item"><div class="k">SHA-256</div><div class="v" style="font-size:11px">${esc(info.sha256 || "—")}</div></div>
    <div class="kv-item"><div class="k">文件有效性</div><div class="v">${info.valid_kdbx ? '<span class="badge active">${IC("check")} 合法 KDBX</span>' : '<span class="badge expired">${IC("close")} 不是 KDBX 文件</span>'}</div></div>`
    : `<div class="empty-hint">密钥库文件不存在：${esc(info.path)}<br>请先在 keepassxc/ 下运行 <code>python init_vault.py</code>，或在设置中修改路径。</div>`;

  // 备份列表
  const bk = await apiGet("/api/vault/backups");
  $("#vault-backups").innerHTML = bk.backups.length
    ? bk.backups.map((b) => `
      <div class="list-item">
        <span>${IC("database")}</span>
        <div class="li-main">
          <div class="li-title">${esc(b.name)}</div>
          <div class="li-sub">${(b.size / 1024).toFixed(1)} KB · ${esc(b.mtime)}</div>
        </div>
        <a class="btn btn-sm" href="/api/vault/backup/${encodeURIComponent(b.name)}">下载</a>
      </div>`).join("")
    : '<div class="empty-hint">暂无备份。上传替换密钥库时，旧文件会自动备份到这里。</div>';

  $("#btn-vault-download").onclick = () => { window.location = "/api/vault/download"; };
  $("#btn-vault-refresh").onclick = () => renderVault();
}

function bindVaultUpload() {
  const drop = $("#vault-drop");
  const input = $("#vault-file");
  const result = $("#vault-upload-result");
  const doUpload = async (file) => {
    result.hidden = false;
    result.className = "result-box";
    result.textContent = `正在上传 ${file.name}（${(file.size / 1024).toFixed(1)} KB）…`;
    try {
      const buf = await file.arrayBuffer();
      const res = await fetch("/api/vault/upload", {
        method: "POST",
        headers: { "Content-Type": "application/octet-stream" },
        body: buf,
      });
      const body = await res.json();
      if (!res.ok) throw new Error(body.error || `HTTP ${res.status}`);
      result.className = "result-box ok";
      result.innerHTML = `${IC("check-circle")} 已替换密钥库<br>新文件 SHA-256：<code>${body.sha256}</code>`;
      await renderVault();
    } catch (e) {
      result.className = "result-box err";
      result.innerHTML = `${IC("close-circle")} 上传失败：${esc(e.message)}`;
    }
  };
  drop.onclick = () => input.click();
  input.onchange = () => { if (input.files[0]) doUpload(input.files[0]); };
  ["dragover", "dragenter"].forEach((ev) => drop.addEventListener(ev, (e) => { e.preventDefault(); drop.classList.add("drag"); }));
  ["dragleave", "drop"].forEach((ev) => drop.addEventListener(ev, (e) => { e.preventDefault(); drop.classList.remove("drag"); }));
  drop.addEventListener("drop", (e) => {
    const file = e.dataTransfer.files && e.dataTransfer.files[0];
    if (file) doUpload(file);
  });
}

/* ==================== 设置 ==================== */
function renderSettings() {
  const s = state.data.settings || {};
  $("#set-name").value = s.name || "账号管家";
  $("#set-vault-path").value = s.vault_path || "";

  // 用量接口配置表
  const ucs = state.usageConfigs || [];
  const tbody = $("#usage-config-tbody");
  tbody.innerHTML = ucs.map((c) => {
    const acc = findAccount(c.account_id);
    const name = acc ? acc.name : "(账号已删除)";
    const cache = c.cache;
    let statusTxt = "—", statusCls = "";
    if (cache) {
      const pct = cache.percent_used;
      if (pct !== null && pct !== undefined) {
        statusTxt = `${pct}%`;
        statusCls = pct >= 90 ? "badge expired" : pct >= 70 ? "badge badge-warn" : "badge active";
      } else if (cache.used !== null && cache.used !== undefined) {
        statusTxt = `used ${cache.used}`;
        statusCls = "badge active";
      }
    }
    const urlShort = c.url.length > 50 ? c.url.slice(0, 50) + "…" : c.url;
    const provBadge = c.provider ? `<span class="badge badge-cat" style="font-size:10px">${esc((state.usageProviders.find(p=>p.key===c.provider)||{}).label || c.provider)}</span>` : "";
    return `
      <tr>
        <td><b>${esc(name)}</b>${acc ? `<div class="li-sub" style="font-size:12px">${esc(acc.vendor || "")}</div>` : ""}</td>
        <td>${provBadge || `<a href="${esc(c.url)}" target="_blank" rel="noopener" style="font-size:12px">${esc(urlShort)}</a>`}<div class="li-sub" style="font-size:11px">${provBadge ? esc(c.url.slice(0,40))+'...' : esc(c.jsonpath_used || "—") + " / " + esc(c.jsonpath_total || "—")}</div></td>
        <td style="white-space:nowrap">${IC("timer-outline")} ${c.interval_min}m</td>
        <td style="font-size:12px">${esc(c.last_run_at ? fmtTimeAgo(c.last_run_at) : "—")}</td>
        <td><span class="${statusCls}">${statusTxt}</span>${c.enabled ? "" : `<div class="li-sub" style="font-size:11px">已停用</div>`}</td>
        <td style="white-space:nowrap">
          <button class="btn btn-sm" data-uc-fetch="${esc(c.id)}" title="立即抓取">${IC("sync")}</button>
          <button class="btn btn-sm" data-ucedit="${esc(c.id)}">编辑</button>
          <button class="btn btn-sm btn-danger" data-ucdel="${esc(c.id)}">删除</button>
        </td>
      </tr>`;
  }).join("");
  $("#usage-config-empty").hidden = ucs.length > 0;

  document.querySelectorAll("[data-ucedit]").forEach((b) => b.onclick = () => openUsageConfigForm(b.dataset.ucedit));
  document.querySelectorAll("[data-ucdel]").forEach((b) => b.onclick = async () => {
    if (!confirm("删除这条用量接口配置？")) return;
    await apiDel(`/api/usage-configs/${b.dataset.ucdel}`);
    await reload();
  });
  document.querySelectorAll("[data-uc-fetch]").forEach((b) => b.onclick = async () => {
    flash("正在抓取…");
    try {
      const r = await apiGet(`/api/usage/fetch?id=${encodeURIComponent(b.dataset.ucFetch)}`);
      if (r.ok) { flash("已抓取"); await reload(); }
      else flash(`抓取失败：${r.error || "未知"}`);
    } catch (e) { flash(`抓取失败：${e.message}`); }
  });

  // 查询链接表
  const qs = state.data.query_links || [];
  $("#query-link-tbody").innerHTML = qs.map((q) => `
    <tr>
      <td>${IC(catInfo(q.category).icon)} ${catInfo(q.category).label}</td>
      <td>${esc(q.vendor || "—")}</td>
      <td>${esc(q.label)}</td>
      <td><a href="${esc(q.url)}" target="_blank" rel="noopener" style="font-size:12px">${esc(q.url)}</a></td>
      <td style="white-space:nowrap">
        <button class="btn btn-sm" data-qedit="${esc(q.id)}">编辑</button>
        <button class="btn btn-sm btn-danger" data-qdel="${esc(q.id)}">删除</button>
      </td>
    </tr>`).join("");
  $("#query-link-empty").hidden = qs.length > 0;

  document.querySelectorAll("[data-qedit]").forEach((b) => b.onclick = () => openQueryLinkForm(b.dataset.qedit));
  document.querySelectorAll("[data-qdel]").forEach((b) => b.onclick = async () => {
    if (!confirm("删除这条查询链接？")) return;
    await apiDel(`/api/query-links/${b.dataset.qdel}`);
    await reload();
  });
}

function openQueryLinkForm(id) {
  const editing = id ? (state.data.query_links || []).find((q) => q.id === id) : null;
  $("#ql-modal-title").textContent = editing ? "编辑查询链接" : "添加查询链接";
  $("#ql-modal-body").innerHTML = `
    <div class="form-grid">
      <label>类别
        <select id="ql-category">${CATEGORY_KEYS.map((k) => `<option value="${k}" ${editing && editing.category === k ? "selected" : ""}>${catInfo(k).label}</option>`).join("")}</select>
      </label>
      <label>厂商（可空，留空则匹配该类别全部）<input class="input-box" id="ql-vendor" value="${esc(editing ? editing.vendor : "")}" placeholder="如 中国移动"></label>
      <label>标签 *<input class="input-box" id="ql-label" value="${esc(editing ? editing.label : "")}" placeholder="如 话费查询 / 用量查询"></label>
      <label class="full">链接 *<input class="input-box" id="ql-url" value="${esc(editing ? editing.url : "")}" placeholder="https://…"></label>
    </div>`;
  $("#ql-modal-backdrop").hidden = false;
  $("#ql-modal-close").onclick = closeQueryLinkForm;
  $("#ql-modal-cancel").onclick = closeQueryLinkForm;
  $("#ql-modal-backdrop").onclick = (e) => { if (e.target === $("#ql-modal-backdrop")) closeQueryLinkForm(); };
  $("#ql-modal-save").onclick = async () => {
    const payload = {
      category: $("#ql-category").value,
      vendor: $("#ql-vendor").value.trim(),
      label: $("#ql-label").value.trim(),
      url: $("#ql-url").value.trim(),
    };
    if (!payload.label || !payload.url) { alert("标签和链接不能为空"); return; }
    try {
      if (editing) await apiPut(`/api/query-links/${editing.id}`, payload);
      else await apiPost("/api/query-links", payload);
    } catch (e) { alert(`保存失败：${e.message}`); return; }
    closeQueryLinkForm();
    await reload();
  };
}
function closeQueryLinkForm() { $("#ql-modal-backdrop").hidden = true; }

/* ==================== 用量接口配置 ==================== */
function openUsageConfigForm(id, preselectProvider) {
  const editing = id ? (state.usageConfigs || []).find((c) => c.id === id) : null;
  const allAiAccs = (state.data.accounts || []).filter((a) => a.category === "ai_member");
  const providers = state.usageProviders || [];
  const c = editing || {};
  // 当前选中的 provider（编辑时用已有值，新建时用 preselectProvider，否则空）
  let curProvider = c.provider || preselectProvider || "";
  let provInfo = providers.find((p) => p.key === curProvider);

  // 根据当前 provider 过滤可选账号
  function accsForProvider(provKey) {
    if (!provKey) return allAiAccs;
    const info = providers.find((p) => p.key === provKey);
    if (info && info.vendor_filter) return allAiAccs.filter((a) => a.vendor === info.vendor_filter);
    return allAiAccs;
  }

  const providerOpts = ['<option value="">自定义（手动填 URL + JSONPath）</option>']
    .concat(providers.map((p) => `<option value="${esc(p.key)}" ${curProvider === p.key ? "selected" : ""}>${esc(p.label)}</option>`))
    .join("");

  $("#uc-modal-body").innerHTML = `
    <div class="form-grid">
      <label class="full">接口类型
        <select id="uc-provider">${providerOpts}</select>
      </label>
      <div class="full" id="uc-provider-desc" style="display:${curProvider ? 'block' : 'none'}">
        ${provInfo ? `<div class="jsonpath-help">${esc(provInfo.description)}${provInfo.default_url ? `<br>自动端点: <code>${esc(provInfo.default_url)}</code>` : ''}</div>` : ''}
      </div>
      <label class="full" id="uc-api-key-label" style="display:${provInfo && provInfo.requires_api_key ? '' : 'none'}">API Key *
        <input class="input-box" id="uc-api-key" type="password" value="${esc(c.api_key || '')}" placeholder="sk-...">
      </label>
      <label class="full">账号 *
        <select id="uc-account">
          ${accsForProvider(curProvider).map((a) => `<option value="${esc(a.id)}" ${c.account_id === a.id ? "selected" : ""}>${esc(a.name)} (${esc(a.vendor || "—")})</option>`).join("")}
        </select>
      </label>
      <label class="full" id="uc-url-label">接口 URL *
        <input class="input-box" id="uc-url" value="${esc(c.url || "")}" placeholder="https://api.example.com/v1/usage">
      </label>
      <label id="uc-method-label">请求方式
        <select id="uc-method">
          <option value="GET" ${(!c.method || c.method === "GET") ? "selected" : ""}>GET</option>
          <option value="POST" ${c.method === "POST" ? "selected" : ""}>POST</option>
        </select>
      </label>
      <label id="uc-interval-label">抓取间隔（分钟）
        <input class="input-box" type="number" min="1" id="uc-interval" value="${esc(c.interval_min || (provInfo ? provInfo.default_interval_min : 60))}">
      </label>
      <label class="full" id="uc-headers-label">请求头（JSON，可空，常用于 Authorization）
        <textarea class="input-box" id="uc-headers" placeholder='{"Authorization": "Bearer sk-..."}'>${esc(c.headers ? JSON.stringify(c.headers, null, 2) : "")}</textarea>
      </label>
      <label class="full" id="uc-body-label">请求体（POST 时发送，可空）
        <textarea class="input-box" id="uc-body" placeholder='{"user_id": "xxx"}'>${esc(c.body || "")}</textarea>
      </label>
      <label id="uc-jp-used-label">used 取值路径 *
        <input class="input-box" id="uc-jp-used" value="${esc(c.jsonpath_used || (provInfo ? provInfo.default_jsonpath_used : ""))}" placeholder="$.data.used">
      </label>
      <label id="uc-jp-total-label">total 取值路径
        <input class="input-box" id="uc-jp-total" value="${esc(c.jsonpath_total || (provInfo ? provInfo.default_jsonpath_total : ""))}" placeholder="$.data.total">
      </label>
      <label id="uc-unit-label">单位（显示用，可空）
        <input class="input-box" id="uc-unit" value="${esc(c.unit || (provInfo ? provInfo.default_unit : ""))}" placeholder="如 h / 次 / $">
      </label>
      <label>启用
        <select id="uc-enabled">
          <option value="1" ${c.enabled !== false ? "selected" : ""}>启用</option>
          <option value="0" ${c.enabled === false ? "selected" : ""}>停用</option>
        </select>
      </label>
      <div class="jsonpath-help full" id="uc-jp-help">
        <b>JSONPath 语法</b>：点号或方括号取值，从响应根开始。<br>
        示例：<code>$.data.usage.used</code> · <code>$['used']</code> · <code>$.items[0].percent</code><br>
        响应示例：<code>{"data":{"used":12.5,"total":40,"unit":"h"}}</code> → used <code>$.data.used</code>，total <code>$.data.total</code>
      </div>
      <div class="full uc-result" id="uc-test-result" hidden></div>
    </div>`;
  $("#uc-modal-backdrop").hidden = false;
  $("#uc-modal-close").onclick = closeUsageConfigForm;
  $("#uc-modal-cancel").onclick = closeUsageConfigForm;
  $("#uc-modal-backdrop").onclick = (e) => { if (e.target === $("#uc-modal-backdrop")) closeUsageConfigForm(); };
  $("#uc-modal-save").onclick = () => saveUsageConfigForm(editing ? editing.id : null);
  $("#uc-modal-test").onclick = () => testUsageConfig();

  // provider 切换时显示/隐藏手动字段、重建账号下拉
  const toggleProviderFields = () => {
    const sel = $("#uc-provider").value;
    const isProv = !!sel && providers.some((p) => p.key === sel);
    const info = providers.find((p) => p.key === sel);
    const needsKey = isProv && info && info.requires_api_key;
    const manualFields = ["#uc-url-label", "#uc-method-label", "#uc-headers-label", "#uc-body-label", "#uc-jp-used-label", "#uc-jp-total-label", "#uc-unit-label", "#uc-jp-help"];
    manualFields.forEach((s) => { const el = $(s); if (el) el.style.display = isProv ? "none" : ""; });
    // API Key 字段：只有 requires_api_key 的 provider 才显示
    const keyLabel = $("#uc-api-key-label");
    if (keyLabel) keyLabel.style.display = needsKey ? "" : "none";
    const descEl = $("#uc-provider-desc");
    if (descEl) {
      descEl.style.display = isProv ? "block" : "none";
      if (isProv && info) {
        let descHtml = esc(info.description);
        if (info.default_url) descHtml += `<br>自动端点: <code>${esc(info.default_url)}</code>`;
        if (info.default_jsonpath_used) descHtml += `<br>取值: <code>${esc(info.default_jsonpath_used)}</code>`;
        descEl.innerHTML = `<div class="jsonpath-help">${descHtml}</div>`;
      }
    }
    // 按 provider 重建账号下拉
    const accSelect = $("#uc-account");
    if (accSelect) {
      const prevVal = accSelect.value;
      const filtered = accsForProvider(sel);
      accSelect.innerHTML = filtered.map((a) => `<option value="${esc(a.id)}">${esc(a.name)} (${esc(a.vendor || "—")})</option>`).join("");
      if (filtered.some((a) => a.id === prevVal)) accSelect.value = prevVal;
    }
    // provider 模式下填默认值（如果用户没改过）
    if (isProv && info) {
      if ($("#uc-jp-used") && !$("#uc-jp-used").value) $("#uc-jp-used").value = info.default_jsonpath_used;
      if ($("#uc-unit") && !$("#uc-unit").value) $("#uc-unit").value = info.default_unit;
      if ($("#uc-interval") && (!$("#uc-interval").value || $("#uc-interval").value == "60")) $("#uc-interval").value = info.default_interval_min;
    }
  };
  $("#uc-provider").onchange = toggleProviderFields;
  toggleProviderFields(); // 初始化
}

function closeUsageConfigForm() { $("#uc-modal-backdrop").hidden = true; }

function readUsageConfigForm() {
  let headers = {};
  const headersRaw = $("#uc-headers").value.trim();
  if (headersRaw) {
    try { headers = JSON.parse(headersRaw); }
    catch { throw new Error("请求头不是合法 JSON"); }
  }
  return {
    provider: $("#uc-provider") ? $("#uc-provider").value : "",
    api_key: $("#uc-api-key") ? $("#uc-api-key").value.trim() : "",
    account_id: $("#uc-account").value,
    url: $("#uc-url").value.trim(),
    method: $("#uc-method").value,
    interval_min: parseInt($("#uc-interval").value, 10) || 60,
    headers,
    body: $("#uc-body").value,
    jsonpath_used: $("#uc-jp-used").value.trim(),
    jsonpath_total: $("#uc-jp-total").value.trim(),
    unit: $("#uc-unit").value.trim(),
    enabled: $("#uc-enabled").value === "1",
  };
}

async function testUsageConfig() {
  const result = $("#uc-test-result");
  let payload;
  try { payload = readUsageConfigForm(); }
  catch (e) { alert(e.message); return; }
  const isProv = !!payload.provider;
  if (!isProv && (!payload.url || (!payload.jsonpath_used && !payload.jsonpath_total))) {
    alert("URL 和至少一个取值路径不能为空");
    return;
  }
  result.hidden = false;
  result.className = "uc-result result-box";
  result.textContent = isProv ? "正在通过内置接口查询…" : "正在测试…";
  try {
    const r = await apiPost("/api/usage-configs/test", payload);
    if (r.ok) {
      const { percent_used, unit, plan_type, credits_balance } = r.result;
      result.className = "uc-result result-box ok";
      const sem = r.result.percent_semantics || "used";
      const pctLabel = percent_used !== null && percent_used !== undefined ? `${percent_used}%（${sem === "used" ? "已用" : "剩余"}）` : "无百分比";
      const extra = [];
      if (plan_type) extra.push(`套餐 ${esc(plan_type)}`);
      if (credits_balance) extra.push(`Credits ${esc(credits_balance)}`);
      result.innerHTML = `${IC("check-circle")} 抓取成功（HTTP ${r.status}）· ${esc(pctLabel)}${extra.length ? " · " + extra.join(" · ") : ""}`;
    } else {
      result.className = "uc-result result-box err";
      result.innerHTML = `${IC("close-circle")} 抓取失败（HTTP ${r.status}）：${esc(r.error)}<br><code>${esc((r.raw || "").slice(0, 200))}</code>`;
    }
  } catch (e) {
    result.className = "uc-result result-box err";
    result.innerHTML = `${IC("close-circle")} ${esc(e.message)}`;
  }
}

async function saveUsageConfigForm(id) {
  let payload;
  try { payload = readUsageConfigForm(); }
  catch (e) { alert(e.message); return; }
  const isProv = !!payload.provider;
  if (!isProv && (!payload.url || (!payload.jsonpath_used && !payload.jsonpath_total))) {
    alert("URL 和至少一个取值路径不能为空");
    return;
  }
  try {
    if (id) await apiPut(`/api/usage-configs/${id}`, payload);
    else await apiPost("/api/usage-configs", payload);
  } catch (e) { alert(`保存失败：${e.message}`); return; }
  closeUsageConfigForm();
  await reload();
  flash(id ? "已更新接口配置" : "已添加接口配置");
}

/* ==================== 顶部操作 ==================== */
function flash(msg) {
  const el = $("#save-state");
  el.textContent = msg;
  setTimeout(() => { if (el.textContent === msg) el.textContent = ""; }, 2500);
}

/* ==================== 初始化 ==================== */
async function init() {
  // 顶部事件
  document.querySelectorAll(".tab").forEach((t) => t.onclick = () => switchTab(t.dataset.tab));
  $("#btn-new-account").onclick = () => openForm(null);
  $("#search-input").oninput = (e) => { state.search = e.target.value; renderAccounts(); };
  $("#status-filter").onchange = (e) => { state.statusFilter = e.target.value; renderAccounts(); };

  // 设置页事件
  $("#btn-save-settings").onclick = async () => {
    await apiPut("/api/settings", {
      name: $("#set-name").value.trim(),
      vault_path: $("#set-vault-path").value.trim(),
    });
    await reload();
    flash("设置已保存");
  };
  $("#btn-new-query-link").onclick = () => openQueryLinkForm(null);
  $("#btn-new-usage-config").onclick = () => openUsageConfigForm(null);
  $("#btn-usage-refresh-all").onclick = async () => {
    const cfgs = state.usageConfigs || [];
    if (!cfgs.length) { flash("没有配置可抓取"); return; }
    flash(`正在抓取 ${cfgs.length} 个接口…`);
    let ok = 0, fail = 0;
    for (const c of cfgs) {
      try {
        const r = await apiGet(`/api/usage/fetch?id=${encodeURIComponent(c.id)}`);
        if (r.ok) ok++; else fail++;
      } catch { fail++; }
    }
    await reload();
    flash(`抓取完成：成功 ${ok}，失败 ${fail}`);
  };
  $("#btn-export").onclick = () => { window.location = "/api/export"; };
  $("#btn-import").onclick = () => $("#import-file").click();
  $("#import-file").onchange = async () => {
    const file = $("#import-file").files[0];
    if (!file) return;
    try {
      const data = JSON.parse(await file.text());
      const accounts = Array.isArray(data.accounts) ? data.accounts : [];
      const relations = Array.isArray(data.relations) ? data.relations : [];
      const links = Array.isArray(data.query_links) ? data.query_links : [];
      const usageCfgs = Array.isArray(data.usage_configs) ? data.usage_configs : [];
      const res = await apiPost("/api/import", { accounts, relations, query_links: links, usage_configs: usageCfgs });
      $("#import-result").hidden = false;
      $("#import-result").className = "result-box ok";
      const imp = res.imported;
      const parts = [`账号 ${imp.accounts} 个`, `关联 ${imp.relations} 条`, `查询链接 ${imp.query_links} 条`];
      if (imp.usage_configs) parts.push(`用量配置 ${imp.usage_configs} 条`);
      $("#import-result").innerHTML = `${IC("check-circle")} 导入完成：${parts.join("，")}`;
      await reload();
    } catch (e) {
      $("#import-result").hidden = false;
      $("#import-result").className = "result-box err";
      $("#import-result").innerHTML = `${IC("close-circle")} 导入失败：${esc(e.message)}`;
    }
    $("#import-file").value = "";
  };
  $("#btn-reset").onclick = async () => {
    if (!confirm("确定清空全部数据？此操作不可撤销（会先自动备份当前数据）。")) return;
    if (!prompt("输入 RESET 确认清空")) return;
    await apiPost("/api/data/reset", { confirm: "RESET" });
    await reload();
    flash("已清空（旧数据已备份）");
  };

  bindVaultUpload();
  await reload();
}

init().catch((e) => alert(`初始化失败：${e.message}`));
