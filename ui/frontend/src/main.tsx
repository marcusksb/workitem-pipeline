import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "@fontsource-variable/inter";
import "@fontsource-variable/jetbrains-mono";
import { App, TerminalPage } from "./App";
import { TooltipProvider } from "@/components/ui/tooltip";
import { installPointerLog } from "./pointerLog";
import "./styles.css";

installPointerLog();

const terminal = new URLSearchParams(location.search).get("terminal");

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <TooltipProvider>{terminal ? <TerminalPage path={terminal} /> : <App />}</TooltipProvider>
  </StrictMode>,
);
