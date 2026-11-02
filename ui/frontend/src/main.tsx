import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App, TerminalPage } from "./App";
import { installPointerLog } from "./pointerLog";
import "./styles.css";

installPointerLog();

const terminal = new URLSearchParams(location.search).get("terminal");

createRoot(document.getElementById("root")!).render(
  <StrictMode>{terminal ? <TerminalPage path={terminal} /> : <App />}</StrictMode>,
);
