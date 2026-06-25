import { createInitialProject } from '../state/initialState'
import type { ProjectState } from '../state/types'

export function normalizeProjectState(project: ProjectState): ProjectState {
  if (!project || typeof project !== 'object') return createInitialProject()
  return project
}
