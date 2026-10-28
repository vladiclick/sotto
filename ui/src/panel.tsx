import React from "react";
import ReactDOM from "react-dom/client";
import Panel from "./components/Panel";
import { isTauri } from "./bridge";
import "./index.css";

// In a plain browser, stand in for the native popover vibrancy behind the page.
if (!isTauri) {
  const dark = window.matchMedia("(prefers-color-scheme: dark)").matches;
  document.body.style.background = dark ? "#2b2926" : "#ecebe7";
  // pin the real window size (browsers have a minimum window width)
  Object.assign(document.getElementById("root")!.style, { width: "320px", height: "468px" });
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <Panel />
  </React.StrictMode>,
);
