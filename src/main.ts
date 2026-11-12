import "./styles.css";
import { isPreview } from "./types";

const indicator =
  (window as { __newtypeIndicator?: boolean }).__newtypeIndicator === true ||
  new URLSearchParams(location.search).get("view") === "indicator";

if (indicator) {
  void import("./indicator").then((module) => module.mount());
} else {
  void import("./settings").then((module) => module.mount(isPreview()));
}
