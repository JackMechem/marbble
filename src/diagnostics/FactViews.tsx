// How the Diagnostics window draws facts.ts's labelled values: a chip in the bar along the top, and
// a card of rows per Overview section. A marked value takes the theme's warning colour.
import type { ReactNode } from "react";
import type { Fact, Section } from "./facts";

const levelClass = (f: Fact) => (f.level ? ` ${f.level}` : "");

/** A figure in the bar along the top: its label over its value. */
export function FactChip({ fact }: { fact: Fact }) {
  return (
    <div className={"diag-chip" + levelClass(fact)} title={fact.hint}>
      <span className="diag-chip-label">{fact.label}</span>
      <span className="diag-chip-value">{fact.value}</span>
    </div>
  );
}

/** An Overview section: a titled card with a row per fact, and anything else it shows below. */
export function FactCard({ section, children }: { section: Section; children?: ReactNode }) {
  return (
    <section className="diag-card" aria-label={section.title}>
      <h2 className="diag-card-title">{section.title}</h2>
      <dl className="diag-facts">
        {section.facts.map((f) => (
          <div key={f.label} className={"diag-fact" + levelClass(f)} title={f.hint}>
            <dt>{f.label}</dt>
            <dd>{f.value}</dd>
          </div>
        ))}
      </dl>
      {children}
    </section>
  );
}
