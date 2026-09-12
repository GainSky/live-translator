import React from "react";
import ReactDOM from "react-dom/client";
import { OverlayApp } from "@/overlay/OverlayApp";
import "@/styles/globals.css";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <OverlayApp />
  </React.StrictMode>,
);
