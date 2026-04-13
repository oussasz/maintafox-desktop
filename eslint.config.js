import tseslint from "@typescript-eslint/eslint-plugin";
import tsparser from "@typescript-eslint/parser";
import reactPlugin from "eslint-plugin-react";
import reactHooks from "eslint-plugin-react-hooks";
import importPlugin from "eslint-plugin-import";

export default [
  {
    ignores: [
      "dist/**",
      "src-tauri/**",
      "node_modules/**",
      "*.config.js",
      "*.config.ts",
      "scripts/**",
    ],
  },
  {
    files: ["src/**/*.{ts,tsx}", "shared/**/*.ts"],
    languageOptions: {
      parser: tsparser,
      parserOptions: {
        project: true,
        tsconfigRootDir: import.meta.dirname,
      },
    },
    plugins: {
      "@typescript-eslint": tseslint,
      react: reactPlugin,
      "react-hooks": reactHooks,
      import: importPlugin,
    },
    settings: {
      react: { version: "detect" },
      "import/resolver": { typescript: { alwaysTryTypes: true } },
    },
    rules: {
      // TypeScript strict rules
      "@typescript-eslint/no-explicit-any": "error",
      "@typescript-eslint/no-floating-promises": "error",
      "@typescript-eslint/no-unused-vars": ["error", { argsIgnorePattern: "^_" }],
      "@typescript-eslint/consistent-type-imports": ["error", { prefer: "type-imports" }],
      "@typescript-eslint/no-non-null-assertion": "warn",

      // React rules
      "react/jsx-uses-react": "off",
      "react/react-in-jsx-scope": "off",
      "react-hooks/rules-of-hooks": "error",
      "react-hooks/exhaustive-deps": "error",

      // Import rules
      "import/no-cycle": "error",
      "import/no-unresolved": "error",
      "import/order": [
        "error",
        {
          "groups": ["builtin", "external", "internal", "parent", "sibling"],
          "pathGroups": [
            { "pattern": "@/**", "group": "internal" },
            { "pattern": "@shared/**", "group": "internal" }
          ],
          "newlines-between": "always",
          "alphabetize": { "order": "asc" }
        }
      ],

      // General quality rules
      "no-console": ["error", { allow: ["warn", "error"] }],
      "no-debugger": "error",
    },
  },
  // Stricter rules for service and store layers: explicit return types required
  {
    files: ["src/services/**/*.ts", "src/store/**/*.ts"],
    rules: {
      "@typescript-eslint/explicit-function-return-type": "warn",
    },
  },
  // No default exports in components, pages, hooks
  {
    files: [
      "src/components/**/*.tsx",
      "src/pages/**/*.tsx",
      "src/hooks/**/*.ts",
    ],
    rules: {
      "import/no-default-export": "error",
    },
  },
];
