// The landing page without this file is complete: a drawn phrase with a still playhead, and download
// links that go to the releases page. This moves the playhead and points the links at the files.

function runPlayhead() {
  const lane = document.querySelector<HTMLElement>("[data-lane]");
  if (!lane || matchMedia("(prefers-reduced-motion: reduce)").matches) return;
  const levels = (lane.dataset.levels ?? "").split(",").map((n) => Number(n) / 99);
  const bars = Number(lane.dataset.bars);
  let x = Number(lane.dataset.start);

  const pos = document.querySelector<HTMLElement>("[data-pos]")!;
  const mode = document.querySelector<HTMLElement>("[data-mode]")!;
  const meters = [...document.querySelectorAll<HTMLElement>("[data-meter]")];
  const peaks = [...document.querySelectorAll<HTMLElement>("[data-peak]")];
  const toggle = document.querySelector<HTMLButtonElement>("[data-toggle]")!;
  const icon = toggle.querySelector("[data-icon]")!;
  const transport = document.querySelector<HTMLElement>("[data-transport]")!;
  transport.hidden = false;

  const BARS_PER_SECOND = 0.75;

  const meterLevel = [0, 0];
  const peakLevel = [0, 0];
  const peakAge = [0, 0];
  let wait = 0;
  let last = 0;
  let frame = 0;
  let playing = true;
  let visible = true;
  let lastPos = "";

  const pad = (n: number, width: number) => String(n).padStart(width, "0");

  const tick = (now: number) => {
    frame = requestAnimationFrame(tick);
    const dt = Math.min(0.1, (now - last) / 1000 || 0);
    last = now;
    if (wait > 0) wait -= dt;
    else {
      x += dt * BARS_PER_SECOND;
      if (x >= bars) {
        x = 0;
        wait = 0.5;
      }
    }
    lane.style.setProperty("--p", String(x / bars));

    const bar = Math.floor(x);
    const within = (x - bar) * 4;
    const text = `${pad(bar + 1, 3)}.${Math.floor(within) + 1}.${(Math.floor(within * 4) % 4) + 1}`;
    if (text !== lastPos) {
      pos.textContent = text;
      mode.textContent = wait > 0 ? "Stop" : "Play";
      mode.classList.toggle("on", wait <= 0);
      lastPos = text;
    }

    const level = wait > 0 ? 0 : levels[Math.floor((x / bars) * levels.length)] ?? 0;
    meters.forEach((m, c) => {
      const wobble = c === 0 ? 1 : 0.94 + 0.05 * Math.sin(now / 90);
      const target = Math.sqrt(level) * wobble;
      // Meters rise at once and fall slowly, as a peak meter does.
      meterLevel[c] = Math.max(target, meterLevel[c] - dt * 1.3);
      m.style.setProperty("--v", meterLevel[c].toFixed(3));
      if (meterLevel[c] >= peakLevel[c]) {
        peakLevel[c] = meterLevel[c];
        peakAge[c] = 0;
      } else if ((peakAge[c] += dt) > 1) peakLevel[c] = Math.max(0, peakLevel[c] - dt * 0.5);
      peaks[c].style.bottom = `${(peakLevel[c] * 100).toFixed(1)}%`;
    });
  };

  const update = () => {
    cancelAnimationFrame(frame);
    if (playing && visible && !document.hidden) {
      last = performance.now();
      frame = requestAnimationFrame(tick);
    }
  };

  toggle.addEventListener("click", () => {
    playing = !playing;
    toggle.setAttribute("aria-label", playing ? "Pause the animation" : "Play the animation");
    icon.setAttribute("d", playing ? "M2 1h3v10H2zM7 1h3v10H7z" : "M2 1l9 5-9 5z");
    update();
  });
  new IntersectionObserver((entries) => {
    visible = entries[0].isIntersecting;
    update();
  }).observe(lane);
  document.addEventListener("visibilitychange", update);
  update();
}

interface Release {
  tag_name: string;
  html_url: string;
  draft: boolean;
  prerelease: boolean;
  assets: { name: string; browser_download_url: string }[];
}

const CHANNEL_NOTES = {
  stable: "Releases with a plain version: the most tested.",
  unstable: "Every release, prereleases included: new features sooner, with less testing.",
};

/**
 * Points the download buttons at the newest release's files on the chosen channel, once they're
 * about to be seen. Stable is the newest release that isn't a prerelease, Unstable the newest of
 * all; the switch starts on Stable when there is one.
 */
function watchDownloads() {
  const section = document.querySelector<HTMLElement>("[data-releases]");
  if (!section) return;
  const show = (latest: Release) => {
    const label = document.querySelector<HTMLAnchorElement>("[data-latest]");
    if (label) {
      label.textContent = latest.tag_name;
      label.href = latest.html_url;
    }
    const kind = document.querySelector<HTMLElement>("[data-kind]");
    if (kind) kind.textContent = latest.prerelease ? "a prerelease" : "a stable release";
    section.querySelectorAll<HTMLAnchorElement>("[data-asset]").forEach((link) => {
      const pattern = new RegExp(link.dataset.asset!);
      const asset = latest.assets.find((a) => pattern.test(a.name));
      link.href = asset?.browser_download_url ?? latest.html_url;
      link.title = asset?.name ?? "";
    });
  };
  const load = async () => {
    try {
      const response = await fetch(section.dataset.releases!, { headers: { Accept: "application/vnd.github+json" } });
      if (!response.ok) return;
      const releases: Release[] = (await response.json()).filter((r: Release) => !r.draft && r.assets.length);
      const newest = { stable: releases.find((r) => !r.prerelease), unstable: releases[0] };
      if (!newest.unstable) return;
      const switcher = section.querySelector<HTMLElement>("[data-channels]");
      const note = section.querySelector<HTMLElement>("[data-channel-note]");
      const buttons = [...section.querySelectorAll<HTMLButtonElement>("[data-channel]")];
      const choose = (channel: keyof typeof newest) => {
        for (const b of buttons) b.setAttribute("aria-checked", String(b.dataset.channel === channel));
        if (note) note.textContent = CHANNEL_NOTES[channel] + (newest.stable ? "" : " There's no stable release yet.");
        show(newest[channel]!);
      };
      for (const b of buttons) {
        const channel = b.dataset.channel as keyof typeof newest;
        if (!newest[channel]) {
          b.disabled = true;
          b.title = "No stable release yet";
        }
        b.addEventListener("click", () => choose(channel));
      }
      if (switcher) switcher.hidden = false;
      choose(newest.stable ? "stable" : "unstable");
    } catch {
      // Offline, or over GitHub's rate limit: the links stay on the releases page.
    }
  };
  const observer = new IntersectionObserver(
    (entries) => {
      if (!entries[0].isIntersecting) return;
      observer.disconnect();
      void load();
    },
    { rootMargin: "400px" },
  );
  observer.observe(section);
}

runPlayhead();
watchDownloads();
