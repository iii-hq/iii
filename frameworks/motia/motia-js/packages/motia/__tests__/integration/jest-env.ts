if (!process.env.III_URL) {
  const port = process.env.TEST_ENGINE_PORT ?? '49199'
  process.env.III_URL = `ws://localhost:${port}`
}
