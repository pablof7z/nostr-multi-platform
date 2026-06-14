import { render } from "solid-js/web";
import App from "./App";
import "./tokens.css";
import "./connection.css";
import "./styles.css";
import "./embed-cards.css";

const root = document.getElementById("root");

if (!root) {
  throw new Error("missing root element");
}

render(() => <App />, root);
