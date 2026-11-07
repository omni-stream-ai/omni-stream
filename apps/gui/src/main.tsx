import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { BrowserRouter, Route, Routes } from 'react-router-dom';
import App from './App';
import { I18nProvider } from './i18n';
import './styles.css';
import { initializeAnalytics } from './lib/firebase';

void initializeAnalytics();

const queryClient = new QueryClient({
  defaultOptions: {
    queries: { staleTime: 2_000, retry: false, refetchOnWindowFocus: false, refetchOnReconnect: false, retryOnMount: false },
  },
});

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <I18nProvider>
        <BrowserRouter>
          <Routes>
            <Route path="*" element={<App />} />
          </Routes>
        </BrowserRouter>
      </I18nProvider>
    </QueryClientProvider>
  </StrictMode>,
);
