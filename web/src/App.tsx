import React, { lazy } from 'react';
import { BrowserRouter, Routes, Route, Navigate } from 'react-router-dom';
import Layout from './components/Layout';

// Lazy load all pages for code splitting
const Welcome = lazy(() => import('./pages/Welcome'));
const Init = lazy(() => import('./pages/Init'));
const Dashboard = lazy(() => import('./pages/Dashboard'));
const Tools = lazy(() => import('./pages/config/Tools'));
const Standards = lazy(() => import('./pages/config/Standards'));
const Skills = lazy(() => import('./pages/config/Skills'));
const Settings = lazy(() => import('./pages/config/Settings'));
const Prd = lazy(() => import('./pages/Prd'));
const Plan = lazy(() => import('./pages/Plan'));
const Apply = lazy(() => import('./pages/Apply'));
const Console = lazy(() => import('./pages/ops/Console'));
const Registry = lazy(() => import('./pages/ops/Registry'));
const Live = lazy(() => import('./pages/ops/Live'));
const Locks = lazy(() => import('./pages/ops/Locks'));
const Diagnostics = lazy(() => import('./pages/ops/Diagnostics'));
const Logs = lazy(() => import('./pages/ops/Logs'));
const Worktrees = lazy(() => import('./pages/ops/Worktrees'));
const Backups = lazy(() => import('./pages/ops/Backups'));
const Git = lazy(() => import('./pages/ops/Git'));
const Help = lazy(() => import('./pages/Help'));
const About = lazy(() => import('./pages/About'));

const App: React.FC = () => {
  return (
    <BrowserRouter>
      <Routes>
        <Route path="/" element={<Layout />}>
          {/* Index route redirects to /dashboard */}
          <Route index element={<Navigate to="/dashboard" replace />} />
          
          <Route path="welcome" element={<Welcome />} />
          <Route path="init" element={<Init />} />
          <Route path="dashboard" element={<Dashboard />} />
          
          {/* Config group */}
          <Route path="config">
            <Route path="tools" element={<Tools />} />
            <Route path="standards" element={<Standards />} />
            <Route path="skills" element={<Skills />} />
            <Route path="settings" element={<Settings />} />
          </Route>

          {/* Workflow stages */}
          <Route path="prd" element={<Prd />} />
          <Route path="plan" element={<Plan />} />
          <Route path="apply" element={<Apply />} />

          {/* Ops group */}
          <Route path="ops">
            <Route path="console" element={<Console />} />
            <Route path="registry" element={<Registry />} />
            <Route path="worktrees" element={<Worktrees />} />
            <Route path="worktrees/create" element={<div className="p-10 text-center">Worktree Creation Wizard - Coming Soon</div>} />
            <Route path="worktrees/:id" element={<div className="p-10 text-center">Worktree Details & Terminal - Coming Soon</div>} />
            <Route path="live" element={<Live />} />
            <Route path="locks" element={<Locks />} />
            <Route path="diagnostics" element={<Diagnostics />} />
            <Route path="logs" element={<Logs />} />
            <Route path="backups" element={<Backups />} />
            <Route path="git" element={<Git />} />
          </Route>

          {/* Utility / Info */}
          <Route path="help" element={<Help />} />
          <Route path="about" element={<About />} />
          
          {/* Catch-all route redirects back to /dashboard */}
          <Route path="*" element={<Navigate to="/dashboard" replace />} />
        </Route>
      </Routes>
    </BrowserRouter>
  );
};

export default App;
