import { createTheme, rem } from '@mantine/core';

export const theme = createTheme({
  primaryColor: 'opapBlue',
  defaultRadius: 'md',
  fontFamily:
    'Inter, ui-sans-serif, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif',
  headings: {
    fontFamily:
      'Inter, ui-sans-serif, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif',
    fontWeight: '650',
  },
  focusRing: 'auto',
  cursorType: 'pointer',
  fontSizes: {
    xs: rem(12),
    sm: rem(13),
    md: rem(15),
    lg: rem(17),
    xl: rem(20),
  },
  colors: {
    gray: [
      '#f8f9fb',
      '#f1f3f5',
      '#e7ebf0',
      '#d8dee5',
      '#b8c1cc',
      '#7c8794',
      '#5c6875',
      '#46515d',
      '#2c3540',
      '#17202a',
    ],
    opapBlue: [
      '#eff8ff',
      '#dff0ff',
      '#b8ddfb',
      '#8bc8f5',
      '#62b3ef',
      '#409fe9',
      '#2589d6',
      '#1873b5',
      '#155f92',
      '#154f76',
    ],
    opapTeal: [
      '#edfcf9',
      '#d4f7ef',
      '#a8eadb',
      '#75dcc5',
      '#42caae',
      '#24b699',
      '#15927c',
      '#117563',
      '#105e51',
      '#104d43',
    ],
  },
  components: {
    Button: {
      defaultProps: { radius: 'md' },
    },
    Paper: {
      defaultProps: { radius: 'lg' },
    },
  },
});
