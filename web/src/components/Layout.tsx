import React from 'react';
import { Link, Outlet } from 'react-router-dom';

const Layout: React.FC = () => {
  return (
    <div className="flex min-h-screen">
      <nav className="w-[200px] bg-gray-100 p-5 border-r border-gray-300">
        <ul className="list-none p-0">
          <li className="mb-2.5">
            <Link to="/">Dashboard</Link>
          </li>
          <li className="mb-2.5">
            <Link to="/logs">Logs</Link>
          </li>
        </ul>
      </nav>
      <main className="flex-grow p-5">
        <Outlet />
      </main>
    </div>
  );
};

export default Layout;
