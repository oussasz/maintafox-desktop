// Applies the stored colour mode before the first paint (no flash of the wrong theme).
// Loaded as a blocking script from index.html (the CSP allows 'self' scripts only).
// Keep the key and values in sync with src/components/ui/ThemeProvider.tsx.
(function () {
  var pref = "system";
  try {
    var saved = localStorage.getItem("maintafox:theme:v2");
    if (saved === "dark" || saved === "light" || saved === "system") pref = saved;
  } catch (e) {
    // storage blocked: follow the system
  }
  var dark =
    pref === "dark" ||
    (pref === "system" &&
      typeof window.matchMedia === "function" &&
      window.matchMedia("(prefers-color-scheme: dark)").matches);
  var root = document.documentElement;
  root.classList.remove("light", "dark");
  root.classList.add(dark ? "dark" : "light");
  root.style.colorScheme = dark ? "dark" : "light";
})();
