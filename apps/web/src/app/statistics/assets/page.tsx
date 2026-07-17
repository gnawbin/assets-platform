import React from 'react';
import Layout from '@/components/Layout';

const PlaceholderPage: React.FC = () => {
  const path = "";
  return (
    <Layout>
      <h1>页面建设中</h1>
      <p>路径: {path}</p>
      <p>此页面正在开发中...</p>
    </Layout>
  );
};

export default PlaceholderPage;
