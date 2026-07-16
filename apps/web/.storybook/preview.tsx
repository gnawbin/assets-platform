import type { Preview } from '@storybook/react';
import { withThemeFromJSXProvider } from '@storybook/addon-themes';
import { MantineProvider, createTheme } from '@mantine/core';
import '@mantine/core/styles.css';
import React from 'react';

const theme = createTheme({
  primaryColor: 'blue',
  defaultRadius: 'md',
  fontFamily:
    '-apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif, "Apple Color Emoji", "Segoe UI Emoji"',
});

const preview: Preview = {
  parameters: {
    controls: {
      matchers: {
        color: /(background|color)$/i,
        date: /Date$/i,
      },
    },
    backgrounds: {
      default: 'light',
      values: [
        { name: 'light', value: '#ffffff' },
        { name: 'dark', value: '#1a1b1e' },
      ],
    },
    layout: 'fullscreen',
  },

  decorators: [
    withThemeFromJSXProvider({
      Provider: ({ children }: { children: React.ReactNode }) => (
        <MantineProvider theme={theme} defaultColorScheme="light">
          {children}
        </MantineProvider>
      ),
    }),
  ],
};

export default preview;
