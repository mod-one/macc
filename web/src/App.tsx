import React from 'react';
import { BrowserRouter, Routes, Route } from 'react-router-dom';
import Layout from './components/Layout';
import Dashboard from './pages/Dashboard';
import Logs from './pages/Logs';

const App: React.FC = () => {
  return (
    <BrowserRouter>
      <div className="bg-blue-500 text-white p-4 text-center font-bold">
        Tailwind CSS is Working!
      </div>
      <Routes>
        <Route path="/" element={<Layout />}>
          <Route index element={<Dashboard />} />
          <Route path="logs" element={<Logs />} />
        </Route>
      </Routes>
    </BrowserRouter>
  );
};

export default App;
