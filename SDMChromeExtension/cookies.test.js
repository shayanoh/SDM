import assert from "node:assert/strict";
import test from "node:test";

import {
  cookiesToSend,
  dedupeCookies,
  domainMatches,
  pickSiteList,
} from "./cookies.js";

const LIST = ["youtube.com", "google.com", "bbc.co.uk"];

test("domainMatches covers subdomains and parents but not look-alikes", () => {
  const table = [
    [".youtube.com", true],
    ["music.youtube.com", true],
    ["YOUTUBE.COM", true],
    ["accounts.google.com", true],
    [".bbc.co.uk", true],
    ["notyoutube.com", false],
    ["youtube.com.evil.net", false],
    ["evilgoogle.com", false],
    [".com", false],
    ["", false],
    [".", false],
    [undefined, false],
  ];
  for (const [domain, expected] of table) {
    assert.equal(domainMatches(domain, LIST), expected, String(domain));
  }
});

const cookie = (domain, name, value = "v") => ({
  domain,
  name,
  value,
  path: "/",
  secure: true,
  httpOnly: false,
  expirationDate: 1893456000,
  hostOnly: false,
  sameSite: "no_restriction",
  storeId: "0",
});

test("cookiesToSend keeps only listed sites and sends only the fields the host uses", () => {
  const sent = cookiesToSend(
    [cookie(".youtube.com", "SID"), cookie("notyoutube.com", "X")],
    LIST,
  );
  assert.deepEqual(sent, [
    {
      domain: ".youtube.com",
      name: "SID",
      value: "v",
      path: "/",
      secure: true,
      httpOnly: false,
      expirationDate: 1893456000,
    },
  ]);
});

test("cookiesToSend drops values that would break a cookies.txt line", () => {
  const sent = cookiesToSend(
    [
      cookie(".youtube.com", "tab", "a\tb"),
      cookie(".youtube.com", "nl", "a\nb"),
      cookie(".youtube.com", "cr", "a\rb"),
      cookie(".youtube.com", "ok", "fine; =#"),
    ],
    LIST,
  );
  assert.deepEqual(
    sent.map((c) => c.name),
    ["ok"],
  );
});

test("a session cookie has no expirationDate key", () => {
  const session = cookie(".youtube.com", "S");
  delete session.expirationDate;
  const [sent] = cookiesToSend([session], LIST);
  assert.equal("expirationDate" in sent, false);
});

test("dedupeCookies keeps one cookie per domain, path and name", () => {
  const deduped = dedupeCookies([
    cookie(".youtube.com", "a", "1"),
    cookie(".youtube.com", "a", "2"),
    cookie("youtube.com", "a", "3"),
  ]);
  assert.equal(deduped.length, 2);
  assert.equal(deduped[0].value, "2", "the later one wins");
});

test("pickSiteList prefers a received list, then the cache, then the bundle", () => {
  const bundled = { version: "b", domains: ["bundled.com"] };
  const cached = { version: "c", domains: ["cached.com"] };
  const received = { version: "r", domains: ["received.com"] };
  assert.equal(pickSiteList(received, cached, bundled), received);
  assert.equal(pickSiteList(undefined, cached, bundled), cached);
  assert.equal(pickSiteList(undefined, undefined, bundled), bundled);
  assert.equal(
    pickSiteList({ version: "x", domains: [] }, cached, bundled),
    cached,
    "an empty list is not a list",
  );
  assert.equal(pickSiteList({ domains: ["a.com"] }, cached, bundled), cached);
});
