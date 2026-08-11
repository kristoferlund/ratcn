function showError(message, error) {
  document.body.textContent = message;
  document.body.style.color = "#ff6b6b";
  console.error(error || message);
}

window.addEventListener("error", (event) => {
  showError(event.message, event.error);
});

window.addEventListener("unhandledrejection", (event) => {
  showError(String(event.reason), event.reason);
});
