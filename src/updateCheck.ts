// The update check's rules, kept pure so they can be tested: reading release tags as versions,
// semantic version order, the newest release on each channel and what the check tells you.
// updates.ts asks GitHub and shows the result.

/** Settings › General › Update Channel. Stable is the final releases; Unstable adds the
 *  prereleases (alphas, betas, release candidates), so it's never behind Stable. */
export type UpdateChannel = "stable" | "unstable";

export const CHANNEL_NAMES: Record<UpdateChannel, string> = { stable: "Stable", unstable: "Unstable" };

/** A semantic version: 0.1.0-alpha.7 is [0, 1, 0] with prerelease ["alpha", "7"]. */
export interface Version {
  major: number;
  minor: number;
  patch: number;
  /** The prerelease identifiers; empty for a final release. */
  pre: string[];
}

/** The fields of a GitHub release the check uses (GET /repos/{owner}/{repo}/releases). */
export interface Release {
  tag_name: string;
  html_url: string;
  draft: boolean;
  prerelease: boolean;
}

/** A newer version on the channel, and its release page. */
export interface UpdateOffer {
  version: string;
  url: string;
}

const SEMVER = /^v?(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?(?:\+[0-9A-Za-z.-]+)?$/;

/** Reads "0.1.0-alpha.7" or a tag ("v0.1.0-alpha.7"); build metadata ("+abc") is ignored, as
 *  semver says. Null for anything else. */
export function parseVersion(s: string): Version | null {
  const m = SEMVER.exec(s.trim());
  if (!m) return null;
  return { major: Number(m[1]), minor: Number(m[2]), patch: Number(m[3]), pre: m[4] ? m[4].split(".") : [] };
}

const numeric = (id: string) => /^\d+$/.test(id);

/** Semver precedence (semver.org §11): negative when `a` comes before `b`. A prerelease comes
 *  before its final release; prerelease identifiers compare one by one, numbers as numbers
 *  (alpha.7 < alpha.10) and below words, words in ASCII order (alpha < beta < rc), and a
 *  shorter list first when the rest is equal. */
export function compareVersions(a: Version, b: Version): number {
  const core = a.major - b.major || a.minor - b.minor || a.patch - b.patch;
  if (core) return Math.sign(core);
  if (!a.pre.length || !b.pre.length) return Math.sign(b.pre.length - a.pre.length);
  for (let i = 0; i < Math.min(a.pre.length, b.pre.length); i++) {
    const x = a.pre[i];
    const y = b.pre[i];
    if (x === y) continue;
    if (numeric(x) && numeric(y)) return Math.sign(Number(x) - Number(y));
    if (numeric(x) !== numeric(y)) return numeric(x) ? -1 : 1;
    return x < y ? -1 : 1;
  }
  return Math.sign(a.pre.length - b.pre.length);
}

/** The channel the check uses. Until one is chosen it follows the build: Unstable while you run
 *  a prerelease (every release so far is one, so Stable would offer nothing), else Stable. */
export function effectiveChannel(stored: string | null | undefined, running: string): UpdateChannel {
  if (stored === "stable" || stored === "unstable") return stored;
  return parseVersion(running)?.pre.length ? "unstable" : "stable";
}

/** The newest release on a channel. Drafts never count, nor tags that aren't versions; Stable
 *  takes only final releases, Unstable every release. */
export function newestOnChannel(releases: unknown, channel: UpdateChannel): (UpdateOffer & { parsed: Version }) | null {
  if (!Array.isArray(releases)) return null;
  let best: (UpdateOffer & { parsed: Version }) | null = null;
  for (const r of releases as Partial<Release>[]) {
    if (!r || typeof r.tag_name !== "string" || typeof r.html_url !== "string" || r.draft) continue;
    const v = parseVersion(r.tag_name);
    if (!v || (channel === "stable" && (r.prerelease || v.pre.length))) continue;
    if (!best || compareVersions(v, best.parsed) > 0) best = { version: r.tag_name.replace(/^v/, ""), url: r.html_url, parsed: v };
  }
  return best;
}

/** The release to offer: the channel's newest when it's newer than the running version. */
export function updateOffer(releases: unknown, channel: UpdateChannel, running: string): UpdateOffer | null {
  const current = parseVersion(running);
  const newest = newestOnChannel(releases, channel);
  if (!current || !newest || compareVersions(newest.parsed, current) <= 0) return null;
  return { version: newest.version, url: newest.url };
}

/** The notice's text. */
export const offerText = (o: UpdateOffer) => `Marbble ${o.version} is out`;

/** What Help › Check for Updates… says when there's nothing newer. A channel with no release
 *  at all (Stable, for now) says so rather than calling a prerelease its newest version. */
export function upToDateText(releases: unknown, channel: UpdateChannel, running: string): string {
  const name = CHANNEL_NAMES[channel];
  if (!newestOnChannel(releases, channel)) return `There's no release on the ${name} channel yet. You're running Marbble ${running}.`;
  return `Marbble ${running} is the newest version on the ${name} channel.`;
}

export const failedText = (reason: string) => `Couldn't check for updates: ${reason.replace(/\.$/, "")}.`;

/** Why GitHub's answer was refused, for `failedText`. `remaining` is the X-RateLimit-Remaining
 *  header: GitHub allows 60 unauthenticated requests an hour from one address. */
export function httpFailure(status: number, remaining: string | null): string {
  if ((status === 403 || status === 429) && remaining === "0") return "GitHub's hourly limit of requests from your address is used up; try again later";
  return `GitHub answered with HTTP ${status}`;
}
