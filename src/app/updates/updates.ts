// The update check: asks GitHub for Marbble's releases once at launch (Settings › General ›
// Check for updates at launch) and from Help › Check for Updates…, and offers a newer one on the
// update channel in a notice (app/updates/UpdateNotice.tsx). It only tells you: View Release
// opens the release's page, and nothing is downloaded or installed. The rules are in
// updateCheck.ts.
import { useEffect, useState, useSyncExternalStore } from "react";
import { call } from "../../engine/commands";
import { getSettings } from "../../settings/appSettings";
import { showToast } from "../../state/store";
import { effectiveChannel, failedText, httpFailure, updateOffer, upToDateText, type UpdateOffer } from "./updateCheck";

/** The latest releases, newest first. */
const RELEASES = "https://api.github.com/repos/JackMechem/marbble/releases?per_page=30";
const TIMEOUT_MS = 15000;

let running: string | null = null;
let asked: Promise<string | null> | null = null;

/** The running version (a debug build can fake it with `MARBBLE_FAKE_VERSION`). */
function runningVersion(): Promise<string | null> {
  asked ??= call<string>("app_version").then((v) => (running = v));
  return asked;
}

/** The running version, once known (Settings › General shows the channel it implies). */
export function useRunningVersion(): string | null {
  const [v, setV] = useState(running);
  useEffect(() => {
    if (v === null) runningVersion().then(setV);
  }, [v]);
  return v;
}

let offer: UpdateOffer | null = null;
const listeners = new Set<() => void>();

function setOffer(o: UpdateOffer | null) {
  offer = o;
  listeners.forEach((l) => l());
}

/** The release the notice offers, or null when there's none (or it was put off). */
export function useUpdateOffer(): UpdateOffer | null {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => offer,
  );
}

/** Later: the notice goes until the next launch's check (or Check for Updates…). */
export const dismissUpdate = () => setOffer(null);

/** View Release: the release's page in the browser. */
export function viewRelease() {
  if (offer) call("open_release_page", { url: offer.url });
  setOffer(null);
}

/** GitHub's list of releases, or why it couldn't be had. */
async function fetchReleases(): Promise<unknown> {
  const abort = new AbortController();
  const timer = window.setTimeout(() => abort.abort(), TIMEOUT_MS);
  try {
    const res = await fetch(RELEASES, { headers: { Accept: "application/vnd.github+json" }, cache: "no-store", signal: abort.signal });
    if (!res.ok) throw httpFailure(res.status, res.headers.get("x-ratelimit-remaining"));
    try {
      return await res.json();
    } catch {
      throw "GitHub's answer couldn't be read";
    }
  } catch (e) {
    if (typeof e === "string") throw e;
    // fetch rejects with a TypeError when there's no connection; its message differs by engine.
    throw abort.signal.aborted ? `GitHub didn't answer within ${TIMEOUT_MS / 1000} seconds` : "couldn't reach GitHub";
  } finally {
    window.clearTimeout(timer);
  }
}

/**
 * Asks GitHub whether a newer version is out on the update channel and shows the notice if so.
 * At launch (`manual` false) that's all it does: a failure is only logged. Help › Check for
 * Updates… also says when there's nothing newer, and why a check failed.
 */
export async function checkForUpdates(manual: boolean) {
  const version = await runningVersion();
  if (!version) return;
  const channel = effectiveChannel(getSettings().ui.update_channel, version);
  let releases: unknown;
  try {
    releases = await fetchReleases();
  } catch (reason) {
    if (manual) showToast(failedText(String(reason)), 6000);
    else console.warn(`Update check: ${reason}`);
    return;
  }
  const found = updateOffer(releases, channel, version);
  if (found) setOffer(found);
  else if (manual) showToast(upToDateText(releases, channel, version), 5000);
}

/** The check at launch, unless Settings › General turns it off. The version is asked for
 *  either way, so Settings › General shows the right channel from the start. */
export function checkUpdatesAtLaunch() {
  if (getSettings().ui.check_updates !== false) checkForUpdates(false);
  else runningVersion();
}
