/**
 * Role: per-page React error boundary. A render crash inside one page must
 * not unmount the whole app (sidebar shell keeps working); the boundary shows
 * an inline error card instead. App keys the wrapper by route, so navigating
 * to another page remounts this component and clears the error state.
 */
import { Component, type ReactNode } from "react"

interface ErrorBoundaryProps { children: ReactNode }
interface ErrorBoundaryState { error: Error | null }

export class ErrorBoundary extends Component<ErrorBoundaryProps, ErrorBoundaryState> {
  state: ErrorBoundaryState = { error: null }

  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    return { error }
  }

  componentDidCatch(error: Error) {
    console.error("[agent-panel] page render crashed:", error)
  }

  render() {
    if (!this.state.error) return this.props.children
    return <div className="react-error react-page-crash" role="alert">
      <strong>页面渲染出错，已阻止整页白屏。</strong>
      <code>{this.state.error.message}</code>
      <div className="react-page-crash-actions">
        <button type="button" onClick={() => this.setState({ error: null })}>重试</button>
        <button type="button" onClick={() => window.location.reload()}>刷新页面</button>
      </div>
    </div>
  }
}
