// Picking the newest release on each channel from GitHub's list. The list isn't in version order
// (it once put v0.1.0-alpha.9 before v0.1.0-alpha.10), so releases are compared by version.

export interface Release {
  tag_name: string;
  html_url: string;
  draft: boolean;
  prerelease: boolean;
  assets: { name: string; browser_download_url: string }[];
}

/** Compares two version tags ("v1.2.3", "v1.2.3-alpha.10") as semantic versions: below zero when
 *  `a` is older. A prerelease is older than its plain version, and numeric prerelease parts
 *  compare as numbers, so alpha.10 is newer than alpha.9. */
export function compareVersions(a: string, b: string): number {
  const parse = (tag: string) => {
    const [core, pre] = tag.replace(/^v/, "").split(/-(.*)/s);
    return { core: core.split(".").map(Number), pre: pre ? pre.split(".") : [] };
  };
  const x = parse(a);
  const y = parse(b);
  for (let i = 0; i < 3; i++) {
    const d = (x.core[i] ?? 0) - (y.core[i] ?? 0);
    if (d) return d;
  }
  if (!x.pre.length || !y.pre.length) return y.pre.length - x.pre.length;
  for (let i = 0; i < Math.max(x.pre.length, y.pre.length); i++) {
    const p = x.pre[i];
    const q = y.pre[i];
    if (p === undefined || q === undefined) return p === undefined ? -1 : 1;
    const numeric = /^\d+$/.test(p) && /^\d+$/.test(q);
    const d = numeric ? Number(p) - Number(q) : p < q ? -1 : p > q ? 1 : 0;
    if (d) return d;
  }
  return 0;
}

/** The newest published release with files on each channel: Stable is the newest that isn't a
 *  prerelease, Unstable the newest of all. */
export function newestReleases(list: Release[]): { stable?: Release; unstable?: Release } {
  const releases = list.filter((r) => !r.draft && r.assets.length).sort((a, b) => compareVersions(b.tag_name, a.tag_name));
  return { stable: releases.find((r) => !r.prerelease), unstable: releases[0] };
}
