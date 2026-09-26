import { StrictMode } from "react"
import { createRoot } from "react-dom/client"
import { BrowserRouter } from "react-router-dom"
import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { Toaster } from "sonner"

import "@workspace/ui/globals.css"
import { App } from "./App"
import { ThemeProvider } from "@/components/theme-provider"
import { SearchProvider } from "@/contexts/search-context"
import "@/i18n" // 初始化 i18n

const queryClient = new QueryClient({
  defaultOptions: { queries: { staleTime: 30_000, retry: 1 } },
})

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <BrowserRouter>
        <ThemeProvider>
          <SearchProvider>
            <App />
            <Toaster richColors position="top-right" />
          </SearchProvider>
        </ThemeProvider>
      </BrowserRouter>
    </QueryClientProvider>
  </StrictMode>,
)
