import { BrowserRouter, Routes, Route } from 'react-router-dom';
import { Header } from '@/components/layout/Header';
import { SkillsPage } from '@/pages/SkillsPage';
import { SettingsPage } from '@/pages/SettingsPage';
import { Toaster } from '@/components/ui/sonner';
import { TooltipProvider } from '@/components/ui/tooltip';

function App() {
  return (
    <BrowserRouter>
      <TooltipProvider>
        <div className="flex flex-col h-screen bg-background text-foreground overflow-hidden">
          {/* Top Navigation */}
          <Header />

          {/* Main Content Area */}
          <main className="flex-1 flex flex-col overflow-hidden">
            <Routes>
              <Route path="/" element={<SkillsPage />} />
              <Route path="/settings" element={<SettingsPage />} />
            </Routes>
          </main>

          {/* Toast notifications */}
          <Toaster />
        </div>
      </TooltipProvider>
    </BrowserRouter>
  );
}

export default App;
