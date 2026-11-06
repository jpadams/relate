export function canRunQuery(engineReady: boolean, running: boolean, text: string): boolean {
  return engineReady && !running && text.trim() !== '';
}
