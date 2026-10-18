// Tauri doesn't have a Node.js server to do proper SSR
// so we use adapter-static with a fallback to index.html to put the site in SPA mode
// See: https://svelte.dev/docs/kit/single-page-apps
// See: https://v2.tauri.app/start/frontend/sveltekit/ for more info
import adapter from "@sveltejs/adapter-static";
import { vitePreprocess } from "@sveltejs/vite-plugin-svelte";

/** @type {import('@sveltejs/kit').Config} */
const config = {
  preprocess: vitePreprocess(),
  vitePlugin: {
    // No dev, o CSS de cada componente vai junto do JS ("injected"), sem os módulos virtuais
    // `X.svelte?svelte&type=style&lang.css`. Se o webview pede um deles antes de o componente
    // ser compilado, o plugin devolve o `.svelte` cru como CSS: as regras ficam sem escopo
    // (o `button { width: 100% }` do Suggestions vale para todos os botões) e o Vite guarda
    // essa versão até o arquivo mudar (DECISIONS D43). O build continua com CSS em arquivo.
    emitCss: process.env.NODE_ENV === "production",
  },
  kit: {
    adapter: adapter({
      fallback: "index.html",
    }),
  },
};

export default config;
