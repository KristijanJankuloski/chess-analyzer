import React from "react";
import ReactDOM from "react-dom/client";
import { App } from "./App";
import { tauriApi } from "./api/tauri";
import type { Api } from "./api/types";
import "./styles.css";

async function chooseApi(): Promise<Api> {
  // `?demo` previews the UI in a plain browser with recorded reviews. Development builds only.
  if (import.meta.env.DEV && new URLSearchParams(window.location.search).has("demo")) {
    const { createDemoApi } = await import("./api/fake");
    return createDemoApi();
  }
  return tauriApi;
}

chooseApi().then((api) => {
  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>
      <App api={api} />
    </React.StrictMode>,
  );
});
