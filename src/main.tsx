import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
// Fonts — loaded locally via @fontsource (no CDN)
import '@fontsource/manrope/600.css';
import '@fontsource/manrope/700.css';
import '@fontsource/manrope/800.css';
import '@fontsource/inter/400.css';
import '@fontsource/inter/500.css';
import '@fontsource/inter/600.css';
import './i18n';
import './index.css';
import App from './App.tsx';
import { bootstrapAppPreferences } from './app/bootstrap';
import { prepareInstallWizardSessionMonitoring } from './stores/install-wizard-session';

bootstrapAppPreferences();
prepareInstallWizardSessionMonitoring(window.location.pathname);

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>
);
