// Pure helpers for the cookie sync. No chrome.* calls in here, so they run
// under `node --test`. The matching rule is the same as SDM's Rust one
// (`SiteRegistry::cookie_domain_matches`), which filters again before writing.

// chrome.alarms accepts delays down to 30 seconds in current Chrome.
export const DEBOUNCE_MINUTES = 0.5;

/** True when `domain` equals a listed domain or is a subdomain of one. */
export function domainMatches(domain, list) {
  if (typeof domain !== "string") {
    return false;
  }
  let host = domain.toLowerCase();
  if (host.startsWith(".")) {
    host = host.slice(1);
  }
  if (host === "") {
    return false;
  }
  return list.some((entry) => host === entry || host.endsWith("." + entry));
}

const BREAKS_A_LINE = /[\t\r\n]/;

/**
 * The cookies SDM needs, reduced to the fields the host reads. Cookies of
 * other sites never leave Chrome; neither do ones that could break the file.
 */
export function cookiesToSend(cookies, list) {
  const sent = [];
  for (const cookie of cookies) {
    if (!domainMatches(cookie.domain, list)) {
      continue;
    }
    const fields = [cookie.domain, cookie.name, cookie.value, cookie.path];
    if (fields.some((field) => BREAKS_A_LINE.test(field))) {
      continue;
    }
    const item = {
      domain: cookie.domain,
      name: cookie.name,
      value: cookie.value,
      path: cookie.path,
      secure: cookie.secure === true,
      httpOnly: cookie.httpOnly === true,
    };
    if (typeof cookie.expirationDate === "number") {
      item.expirationDate = cookie.expirationDate;
    }
    sent.push(item);
  }
  return sent;
}

/** One cookie per domain, path and name; the later one wins, order kept. */
export function dedupeCookies(cookies) {
  const byKey = new Map();
  for (const cookie of cookies) {
    byKey.set(`${cookie.domain}\t${cookie.path}\t${cookie.name}`, cookie);
  }
  return [...byKey.values()];
}

function isUsable(list) {
  return (
    list !== undefined &&
    list !== null &&
    typeof list.version === "string" &&
    Array.isArray(list.domains) &&
    list.domains.length > 0
  );
}

/** The newest usable site list: from the host, else the cache, else bundled. */
export function pickSiteList(received, cached, bundled) {
  return [received, cached, bundled].find(isUsable) ?? bundled;
}
