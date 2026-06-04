/**
 * Live page has been merged into Console.
 * This redirect preserves any existing bookmarks or links.
 */
import { Navigate } from 'react-router-dom';

const Live = () => <Navigate to="/ops/console" replace />;
export default Live;
