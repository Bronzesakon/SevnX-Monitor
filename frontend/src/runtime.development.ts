import { installBrowserDevelopmentFallback } from './lib/ipc'
import { developmentSettings, developmentSnapshot } from './development/fixtures'
import { mountApplication as mountProductionApplication } from './runtime.production'

export function mountApplication(): void {
  installBrowserDevelopmentFallback({
    settings: developmentSettings,
    snapshot: developmentSnapshot,
  })
  mountProductionApplication()
}
