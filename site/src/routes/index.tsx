import { readFileSync } from "node:fs";
import { parseCrepus, renderCrepusIr } from "@tschk/crepus-moonshine";

const template = readFileSync(
  new URL("../templates/home.crepus", import.meta.url),
  "utf8",
);

export default function Home() {
  return renderCrepusIr(parseCrepus(template, {}));
}
