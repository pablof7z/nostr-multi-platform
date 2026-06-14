import { chromium } from "@playwright/test";
const b = await chromium.launch(); const p = await b.newPage();
await p.goto("https://nmp-gallery.f7z.io/", { waitUntil:"domcontentloaded" });
const poll = async (sel, ms) => { const s=Date.now(); while(Date.now()-s<ms){ if((await p.locator(sel).count())&&await p.locator(sel).first().isVisible()) return Math.round((Date.now()-s)/1000); await new Promise(r=>setTimeout(r,2000)); } return -1; };
await p.locator('[data-testid="status-bar"]').waitFor({state:"visible",timeout:60000});
const sels = {
  avatar:'[data-testid="avatar-row"] img', name:'[data-testid="name-demo"]', nip05:'[data-testid="nip05-demo"]',
  card:'[data-testid="card-demo"]', npub:'[data-testid="user-npub"]', cnote:'[data-testid="content-note"]',
  minimal:'[data-testid="content-minimal"]', mention:'[data-testid="content-mention-chip"]',
  media:'[data-testid="content-media-grid"] img', quote:'[data-testid="content-quote-card"]',
  article:'[data-testid="embed-article"] .nostr-article-card__author', highlight:'[data-testid="embed-highlight"]',
  kindreg:'[data-testid="content-kind-registry"] .nostr-article-card__title', relay:'[data-testid="relay-list-demo"]',
  login:'[data-testid="login-block-demo"]',
};
const r={};
for(const [k,sel] of Object.entries(sels)) r[k]=await poll(sel, 130000);
const aa = r.article>=0 ? (await p.locator('[data-testid="embed-article"] .nostr-article-card__author').innerText()).trim() : "(none)";
console.log("LIVE:", JSON.stringify(r), "articleAuthor:", aa);
const stuck = Object.entries(r).filter(([k,v])=>v<0).map(([k])=>k);
console.log(stuck.length? "STUCK: "+stuck.join(","): "ALL "+Object.keys(sels).length+" RESOLVED");
await b.close();
