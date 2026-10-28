import React from "react";
import ReactDOM from "react-dom/client";
import Overlay from "./components/Overlay";
import { isTauri } from "./bridge";
import "./index.css";

// In a plain browser, show the pill on a desktop-like backdrop.
if (!isTauri) {
  document.body.style.background = "#6f7d86";
  Object.assign(document.getElementById("root")!.style, { width: "184px", height: "56px" });
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <Overlay />
  </React.StrictMode>,
);
