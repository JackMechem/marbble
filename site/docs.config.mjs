// How the repository's Markdown becomes pages. The Markdown stays the only source of the words;
// this file decides only the sections' grouping and where the long ones are cut.
//
// `parts` groups a document's `##` sections for the sidebar, by section key: the heading's slug
// without its number ("## 5. Editing in the tracks area" is `editing-in-the-tracks-area`). A new
// section nobody has placed still gets a page, in the last part, with a warning.
//
// `split` cuts a section into pages. Each entry starts a page at a heading inside the section,
// named by its GitHub anchor in the source document (`[page title, anchor]`), and the page runs
// until the next entry. A title of null uses the heading's own. An anchor that has gone away
// costs a warning and the pages either side merge; nothing breaks.
//
// `single` keeps a short document on one page, its `##` sections as headings on it.

export const REPO = "JackMechem/marbble";
export const REPO_URL = `https://github.com/${REPO}`;
export const BRANCH = "main";
export const SITE = "https://marbble.dev";
// The site is served from the domain root. A subpath here would be "/name", with no trailing slash.
export const BASE = "";

// Past this many source lines the build prints a warning: time to add a split.
export const LONG_PAGE_LINES = 330;

export const DOCS = [
  {
    area: "guide",
    zone: "guide",
    file: "docs/USER_GUIDE.md",
    title: "User Guide",
    overview: "Overview",
    parts: [
      ["Start here", ["getting-started", "the-main-window"]],
      ["Record and play", ["recording", "software-instruments-and-musical-typing"]],
      [
        "Arrange",
        [
          "editing-in-the-tracks-area",
          "track-stacks-groups-hiding-and-freezing",
          "take-folders-and-comping",
          "markers-and-the-arrangement-track",
          "tempo-and-time-signature",
        ],
      ],
      ["Editors", ["the-piano-roll-event-list-and-step-sequencer", "the-audio-track-editor"]],
      ["Mix", ["mixing", "automation", "controller-assignments-midi-learn"]],
      ["Deliver", ["bounce-and-export", "projects"]],
      ["Reference", ["settings-reference", "key-command-reference", "troubleshooting", "known-limitations"]],
    ],
    split: {
      "software-instruments-and-musical-typing": [
        [null, "marbble-sampler"],
        [null, "marbble-tre"],
        [null, "the-other-synth-models"],
        [null, "musical-typing"],
        [null, "step-input"],
      ],
      "editing-in-the-tracks-area": [
        ["Selecting, moving and splitting", "selecting-regions"],
        ["Tools and loops", "tools-and-the-marquee"],
        ["Fades, crossfades and gain", "fades-and-crossfades"],
        ["Region parameters and commands", "region-playback-parameters"],
        [null, "arranging-commands"],
        ["Importing, tracks and undo", "importing-audio-and-midi"],
      ],
      "the-piano-roll-event-list-and-step-sequencer": [
        ["Piano Roll", "view"],
        [null, "event-list"],
        [null, "step-sequencer"],
      ],
      mixing: [
        ["Channel strip and Channel EQ", "the-channel-strip"],
        [null, "plug-ins"],
        [null, "built-in-effects"],
        ["Dynamics effects", "compressor"],
        ["Space and modulation effects", "reverb"],
        ["Drive, EQ and metering effects", "overdrive"],
        ["Effect presets", "presets"],
        ["Sends, auxes and multi-output", "sends-and-aux-buses"],
        [null, "vca-faders"],
        [null, "plug-in-delay-compensation"],
      ],
    },
  },
  {
    area: "contributing",
    zone: "contributors",
    file: "CONTRIBUTING.md",
    title: "Contributing",
    single: true,
  },
  {
    area: "architecture",
    zone: "contributors",
    file: "docs/ARCHITECTURE.md",
    title: "Architecture",
    overview: "Introduction",
    parts: [
      ["Foundations", ["the-parts", "the-source-tree", "threads", "real-time-rules"]],
      ["Flow", ["the-life-of-an-edit", "the-life-of-a-block"]],
      ["Data", ["the-project-model", "files-on-disk"]],
      ["Engine", ["plug-ins", "audio-devices-and-recording", "midi-and-control-surfaces"]],
      ["Interface", ["the-frontend"]],
      ["Reference", ["invariants-and-gotchas"]],
    ],
    split: {
      "the-source-tree": [
        ["The app and the frontend", "the-app-src-taurisrc"],
      ],
      "the-life-of-a-block": [
        [null, "delay-compensation"],
        [null, "multi-core-processing"],
        [null, "offline-renders"],
      ],
      "the-frontend": [
        ["Keys, menus and windows", "keys-menus-and-sheets"],
      ],
    },
  },
  {
    area: "developing",
    zone: "contributors",
    file: "docs/DEVELOPING.md",
    title: "Developing",
    overview: "Introduction",
    parts: [["", ["set-up", "build-and-run", "tests", "debugging", "conventions", "common-changes", "ci", "packaging-and-releases"]]],
    split: {
      tests: [[null, "headless-checks"]],
      conventions: [[null, "styles-and-the-design-system"]],
    },
  },
  {
    area: "roadmap",
    zone: "roadmap",
    file: "ROADMAP.md",
    title: "Roadmap",
    single: true,
  },
  {
    // Generated by scripts/features.py from features/*.toml: one page per area.
    area: "features",
    zone: "roadmap",
    file: "docs/FEATURES.md",
    title: "Feature list",
    overview: "Summary",
    parts: [
      [
        "",
        [
          "interface-and-settings",
          "projects",
          "transport-and-tempo-sync",
          "recording",
          "tracks",
          "arranging",
          "editors",
          "time-and-pitch",
          "mixing-and-automation",
          "plug-ins",
          "instruments-and-content",
          "midi-and-hardware",
          "import-export-and-media",
        ],
      ],
    ],
  },
  {
    // One page per release, newest first, as the file has them.
    area: "changelog",
    zone: "changelog",
    file: "CHANGELOG.md",
    title: "Changelog",
    releases: true,
  },
];

// The four zones of the docs. The sidebar shows one zone at a time, and the header names them all.
export const ZONES = [
  { id: "guide", label: "User Guide", audience: "For musicians", href: "/guide/", areas: ["guide"] },
  { id: "contributors", label: "Contributing, Architecture, Developing", audience: "For contributors", href: "/contributing/", areas: ["contributing", "architecture", "developing"] },
  { id: "roadmap", label: "Roadmap, Feature list", audience: "What's next", href: "/roadmap/", areas: ["roadmap", "features"] },
  { id: "changelog", label: "Changelog", audience: "What changed", href: "/changelog/", areas: ["changelog"] },
];
