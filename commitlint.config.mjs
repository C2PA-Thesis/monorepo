// Conventional commits. Scopes are the monorepo folders.
export default {
  extends: ['@commitlint/config-conventional'],
  rules: {
    'scope-enum': [
      2,
      'always',
      [
        'c2pa',
        'zklp',
        'hyperveritas',
        'binding',
        'pipeline',
        'web',
        'fixtures',
        'editproof',
        'locproof',
        'docs',
        'ci',
        'repo',
      ],
    ],
    'header-max-length': [2, 'always', 100],
    'body-max-line-length': [1, 'always', 100],
  },
};
