// The sidebar shows one zone of the docs at a time: musicians never scroll past the module map,
// and contributors don't scroll past the key commands.
import { defineRouteMiddleware } from "@astrojs/starlight/route-data";
import { DOCS, ZONES } from "../docs.config.mjs";

export const onRequest = defineRouteMiddleware((context) => {
  const route = context.locals.starlightRoute;
  const area = route.id.split("/")[0];
  const zone = ZONES.find((z) => z.areas.includes(area));
  if (!zone) return;

  const titles = DOCS.filter((d) => zone.areas.includes(d.area)).map((d) => d.title);
  const groups = route.sidebar.filter((e) => e.type === "group" && titles.includes(e.label));
  // One document is its own sidebar; two keep their names as headings.
  route.sidebar = groups.length === 1 && groups[0].type === "group" ? groups[0].entries : groups;

  // Prev and next stay inside the zone.
  const inZone = (link?: { href: string }) => {
    if (!link) return false;
    const path = link.href.replace(/^.*?\/fermata\//, "");
    return zone.areas.some((a) => path.startsWith(`${a}/`));
  };
  const { prev, next } = route.pagination;
  route.pagination = { prev: inZone(prev) ? prev : undefined, next: inZone(next) ? next : undefined };
});
