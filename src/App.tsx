import { MantineProvider } from '@mantine/core';
import Layout from './components/Layout';
import Dashboard from './pages/Dashboard';
import './App.css';

function App() {
  return (
    <MantineProvider>
      <Layout>
        <Dashboard />
      </Layout>
    </MantineProvider>
  );
}

export default App;
