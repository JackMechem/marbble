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

export const REPO = "JackMechem/marbble";
export const REPO_URL = `https://github.com/${REPO}`;
export const BRANCH = "main";
export const SITE = "https://jackmechem.github.io";
export const BASE = "/marbble";

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
    area: "architecture",
    zone: "contributors",
    file: "docs/ARCHITECTURE.md",
    title: "Architecture",
    overview: "Introduction",
    parts: [
      ["Foundations", ["overview", "processes-and-threads", "real-time-rules", "how-an-edit-flows"]],
      ["Data", ["the-project-model", "undo", "persistence", "tempo-map"]],
      ["Engine", ["the-engine-graph-block-by-block", "the-vst3-host", "audio-io-and-latency", "built-in-effects-and-presets"]],
      ["Interface", ["the-frontend"]],
      ["Reference", ["cross-cutting-patterns", "module-map"]],
    ],
    split: {
      "the-project-model": [
        ["Ids, tracks and routing", "ids"],
        ["Regions, takes and notes", "regions"],
        ["Automation, markers and the rest", "automation"],
      ],
      "the-engine-graph-block-by-block": [
        [null, "plug-in-delay-compensation"],
        [null, "multi-core-processing"],
        ["Offline render and Isolate", "offline-render-and-isolate"],
      ],
      "the-vst3-host": [
        ["Loading and scanning", "finding-and-loading-bundles"],
        ["Processing and parameters", "processing"],
        [null, "editor-windows"],
      ],
      "built-in-effects-and-presets": [
        ["The effect framework", "the-effect-framework"],
        [null, "built-in-instruments"],
        ["Marbble Tre internals", "marbble-tre"],
        ["Marbble Sampler internals", "marbble-sampler"],
        ["Synth model internals", "marbble-sei"],
        ["Clipboard and strip settings", "clipboard-and-moving-plug-ins"],
      ],
      "module-map": [
        ["The engine crate", "cratesenginesrc"],
        ["VST3 host and Tauri", "cratesvst3-hostsrc"],
        ["Frontend", "src-frontend"],
        ["Frontend components", "srccomponents"],
        ["Tests and packaging", "tests-examples-and-packaging"],
      ],
    },
  },
  {
    area: "developing",
    zone: "contributors",
    file: "docs/DEVELOPING.md",
    title: "Developing",
    overview: "Introduction",
    parts: [["", ["build-and-run", "tests", "debugging", "conventions", "recipes", "packaging", "versioning-and-releases"]]],
    split: {
      tests: [
        ["Rust unit tests", "rust-unit-tests"],
        ["Headless examples", "headless-examples"],
        ["Frontend tests and what to run", "frontend-tests"],
      ],
    },
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

// The three zones of the docs. The sidebar shows one zone at a time, and the header names them all.
export const ZONES = [
  { id: "guide", label: "User Guide", audience: "For musicians", href: "/guide/", areas: ["guide"] },
  { id: "contributors", label: "Architecture, Developing", audience: "For contributors", href: "/architecture/", areas: ["architecture", "developing"] },
  { id: "changelog", label: "Changelog", audience: "What changed", href: "/changelog/", areas: ["changelog"] },
];
