// `import icon from 'x.svg' with { type: 'text' }` gives the file contents as a string.
declare module '*.svg' {
  const source: string
  export default source
}
