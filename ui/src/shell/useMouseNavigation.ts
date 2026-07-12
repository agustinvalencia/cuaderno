// Wire the mouse's back/forward side buttons to history navigation, the
// way a browser does (user request 2026-07-12). `MouseEvent.button` is 3
// for the "back" side button (X1) and 4 for "forward" (X2). We swallow
// the matching `mousedown` so the webview can't also run its own default
// back/forward (a double step), then navigate on `mouseup`.
import { useEffect } from "react";
import { useNavigate } from "react-router";

const BACK_BUTTON = 3;
const FORWARD_BUTTON = 4;

export function useMouseNavigation() {
  const navigate = useNavigate();
  useEffect(() => {
    function suppress(event: MouseEvent) {
      if (event.button === BACK_BUTTON || event.button === FORWARD_BUTTON) {
        event.preventDefault();
      }
    }
    function onMouseUp(event: MouseEvent) {
      if (event.button === BACK_BUTTON) {
        event.preventDefault();
        navigate(-1);
      } else if (event.button === FORWARD_BUTTON) {
        event.preventDefault();
        navigate(1);
      }
    }
    window.addEventListener("mousedown", suppress);
    window.addEventListener("mouseup", onMouseUp);
    return () => {
      window.removeEventListener("mousedown", suppress);
      window.removeEventListener("mouseup", onMouseUp);
    };
  }, [navigate]);
}
